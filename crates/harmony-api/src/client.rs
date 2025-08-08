use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use rmpv::Value;
use serde::{Deserialize, Serialize};

use async_tungstenite::{tokio::connect_async, tungstenite::protocol::Message};
use tokio::sync::{Mutex, RwLock, mpsc};
use tokio::time::timeout;
use url::Url;
use uuid::Uuid;

use crate::error::{HarmonyError, Result};
use crate::events::{Event, EventHandler, NoOpEventHandler, RpcApiEvent};

/// Configuration for the Harmony client
#[derive(Clone, Debug)]
pub struct ClientOptions {
    /// WebSocket server URL
    pub server_url: String,
    /// Authentication token issued by AS
    pub token: String,
    /// Connection timeout
    pub timeout: Duration,
    /// Whether to automatically reconnect on connection loss
    pub auto_reconnect: bool,
    /// Maximum number of reconnection attempts
    pub max_reconnect_attempts: u32,
}

impl ClientOptions {
    pub fn new(server_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            server_url: server_url.into(),
            token: token.into(),
            timeout: Duration::from_secs(30),
            auto_reconnect: true,
            max_reconnect_attempts: 5,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_auto_reconnect(mut self, enabled: bool) -> Self {
        self.auto_reconnect = enabled;
        self
    }

    pub fn with_max_reconnect_attempts(mut self, attempts: u32) -> Self {
        self.max_reconnect_attempts = attempts;
        self
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
enum RpcApiRequest {
    #[serde(rename_all = "camelCase")]
    Identify { token: String, public_key: Vec<u8> },
    #[serde(rename_all = "camelCase")]
    Message {
        id: String,
        method: String,
        data: Value,
    },
}

#[derive(Debug, Deserialize)]
struct RpcApiResponse {
    #[allow(dead_code)]
    id: Option<String>,
    response: Option<Value>,
}

struct ClientState {
    pending_requests: HashMap<String, mpsc::UnboundedSender<Value>>,
    event_handler: Arc<dyn EventHandler>,
    connected: bool,
    reconnecting: bool,
    reconnect_attempts: u32,
}

#[derive(Clone)]
pub struct HarmonyClient {
    options: ClientOptions,
    state: Arc<RwLock<ClientState>>,
    websocket_tx: Arc<Mutex<Option<mpsc::UnboundedSender<Message>>>>,
    reconnect_tx: mpsc::UnboundedSender<()>,
}

impl HarmonyClient {
    pub async fn new(options: ClientOptions) -> Result<Self> {
        let (reconnect_tx, reconnect_rx) = mpsc::unbounded_channel();

        let client = Self {
            options: options.clone(),
            state: Arc::new(RwLock::new(ClientState {
                pending_requests: HashMap::new(),
                event_handler: Arc::new(NoOpEventHandler),
                connected: false,
                reconnecting: false,
                reconnect_attempts: 0,
            })),
            websocket_tx: Arc::new(Mutex::new(None)),
            reconnect_tx,
        };

        client.spawn_reconnection_handler(reconnect_rx).await;

        client.connect().await?;
        Ok(client)
    }

    pub async fn new_with_handler(
        options: ClientOptions,
        event_handler: Arc<dyn EventHandler>,
    ) -> Result<Self> {
        let (reconnect_tx, reconnect_rx) = mpsc::unbounded_channel();

        let client = Self {
            options: options.clone(),
            state: Arc::new(RwLock::new(ClientState {
                pending_requests: HashMap::new(),
                event_handler,
                connected: false,
                reconnecting: false,
                reconnect_attempts: 0,
            })),
            websocket_tx: Arc::new(Mutex::new(None)),
            reconnect_tx,
        };

        client.spawn_reconnection_handler(reconnect_rx).await;

        client.connect().await?;
        Ok(client)
    }

    async fn connect(&self) -> Result<()> {
        connect(self, false).await
    }

    async fn mark_disconnected(
        state: Arc<RwLock<ClientState>>,
        websocket_tx: Arc<Mutex<Option<mpsc::UnboundedSender<Message>>>>,
    ) {
        {
            let mut websocket_tx = websocket_tx.lock().await;
            *websocket_tx = None;
        }

        {
            let mut state = state.write().await;
            state.connected = false;

            let handler = state.event_handler.clone();
            tokio::spawn(async move {
                handler.on_disconnected();
            });

            for (_, sender) in state.pending_requests.drain() {
                drop(sender);
            }
        }
    }

    async fn spawn_reconnection_handler(&self, mut reconnect_rx: mpsc::UnboundedReceiver<()>) {
        let state = self.state.clone();
        let options = self.options.clone();
        let client = self.clone();

        tokio::spawn(async move {
            while reconnect_rx.recv().await.is_some() {
                let should_reconnect = {
                    let mut state = state.write().await;
                    if !options.auto_reconnect || state.connected || state.reconnecting {
                        continue;
                    }
                    state.reconnecting = true;
                    state.reconnect_attempts = 0;
                    true
                };

                if !should_reconnect {
                    continue;
                }

                let mut attempt = 1;
                let max_attempts = options.max_reconnect_attempts;

                while attempt <= max_attempts {
                    {
                        let mut state = state.write().await;
                        state.reconnect_attempts = attempt;
                    }

                    {
                        let state_read = state.read().await;
                        let handler = state_read.event_handler.clone();
                        drop(state_read);

                        handler.on_reconnecting(attempt, max_attempts);
                    }

                    // Calculate backoff delay: 2^(attempt-1) seconds, max 30 seconds
                    if attempt > 1 {
                        let delay = Duration::from_secs((2_u64.pow((attempt - 1).min(5))).min(30));
                        tokio::time::sleep(delay).await;
                    }
                    match connect(&client.clone(), true).await {
                        Ok(()) => {
                            break;
                        }
                        Err(e) => {
                            eprintln!("Reconnection attempt {} failed: {}", attempt, e);
                            attempt += 1;
                        }
                    }
                }

                {
                    let state_read = state.read().await;
                    if state_read.reconnect_attempts >= max_attempts {
                        {
                            let mut state = state.write().await;
                            state.reconnecting = false;
                        }

                        let handler = state_read.event_handler.clone();
                        drop(state_read);

                        handler.on_reconnection_failed(max_attempts);
                    }
                }
            }
        });
    }

    async fn authenticate(&self) -> Result<()> {
        authenticate(self).await
    }

    async fn handle_binary_message(state: Arc<RwLock<ClientState>>, data: &[u8]) -> Result<()> {
        let mut deserializer = rmp_serde::Deserializer::new(&data[..]);
        let value: Value = match serde::Deserialize::deserialize(&mut deserializer) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Failed to deserialize MessagePack: {}", e);
                return Ok(());
            }
        };

        if let Some(id_value) = value
            .as_map()
            .and_then(|m| m.iter().find(|(k, _)| k.as_str() == Some("id")))
        {
            if let Some(id) = id_value.1.as_str() {
                let mut state_lock = state.write().await;
                if let Some(sender) = state_lock.pending_requests.remove(id) {
                    let _ = sender.send(value);
                    return Ok(());
                }
                // Request doesn't exist
                return Ok(());
            }
        }

        if let Ok(event_wrapper) = rmpv::ext::from_value::<RpcApiEvent>(value.clone()) {
            match event_wrapper {
                RpcApiEvent::Identify {} => {
                    println!("Authentication successful");
                }
                RpcApiEvent::Message { event } => {
                    if let Ok(event) = rmpv::ext::from_value::<Event>(event) {
                        let state_lock = state.read().await;
                        let handler = state_lock.event_handler.clone();
                        drop(state_lock);

                        match event {
                            Event::Connected => handler.on_connected(),
                            Event::Disconnected => handler.on_disconnected(),
                            Event::Reconnecting {
                                attempt,
                                max_attempts,
                            } => handler.on_reconnecting(attempt, max_attempts),
                            Event::Reconnected => handler.on_reconnected(),
                            Event::ReconnectionFailed { attempts } => {
                                handler.on_reconnection_failed(attempts)
                            }
                        }
                    }
                }
                _ => {}
            }
        } else {
            eprintln!("Received unknown message format: {:?}", value);
        }

        Ok(())
    }

    pub(crate) async fn send_request<T, R>(&self, method: &str, params: T) -> Result<R>
    where
        T: Serialize,
        R: for<'de> Deserialize<'de>,
    {
        let request_id = Uuid::new_v4().to_string();

        let params_value = rmpv::ext::to_value(params)
            .map_err(|e| HarmonyError::Internal(format!("Params serialization error: {}", e)))?;

        let request = RpcApiRequest::Message {
            id: request_id.clone(),
            method: method.to_string(),
            data: params_value,
        };

        let mut buf = Vec::new();
        request
            .serialize(&mut rmp_serde::Serializer::new(&mut buf).with_struct_map())
            .map_err(|e| HarmonyError::Internal(format!("Serialization error: {}", e)))?;

        let (tx, mut rx) = mpsc::unbounded_channel();

        {
            let mut state = self.state.write().await;
            if !state.connected {
                if state.reconnecting {
                    return Err(HarmonyError::Reconnecting);
                } else {
                    return Err(HarmonyError::NotConnected);
                }
            }
            state.pending_requests.insert(request_id.clone(), tx);
        }

        {
            let websocket_tx = self.websocket_tx.lock().await;
            if let Some(sender) = websocket_tx.as_ref() {
                sender
                    .send(Message::binary(buf))
                    .map_err(|_| HarmonyError::ConnectionLost)?;
            } else {
                return Err(HarmonyError::NotConnected);
            }
        }

        let response_value = timeout(self.options.timeout, rx.recv())
            .await
            .map_err(|_| {
                let state = self.state.clone();
                let request_id = request_id.clone();
                tokio::spawn(async move {
                    let mut state = state.write().await;
                    state.pending_requests.remove(&request_id);
                });
                HarmonyError::Internal("Request timeout".to_string())
            })?
            .ok_or_else(|| HarmonyError::Internal("Response channel closed".to_string()))?;

        let response: RpcApiResponse = rmpv::ext::from_value(response_value)?;

        if let Some(response_data) = response.response {
            let result: R = rmpv::ext::from_value(response_data)?;
            Ok(result)
        } else {
            Err(HarmonyError::Internal("Missing response data".to_string()))
        }
    }

    pub async fn is_connected(&self) -> bool {
        let state = self.state.read().await;
        state.connected
    }

    pub async fn is_reconnecting(&self) -> bool {
        let state = self.state.read().await;
        state.reconnecting
    }

    pub async fn reconnect_attempts(&self) -> u32 {
        let state = self.state.read().await;
        state.reconnect_attempts
    }

    pub async fn reconnect(&self) -> Result<()> {
        let is_connected = {
            let state = self.state.read().await;
            state.connected
        };

        if is_connected {
            return Err(HarmonyError::Internal("Already connected".to_string()));
        }

        connect(self, true).await
    }

    pub async fn set_event_handler(&self, handler: Arc<dyn EventHandler>) {
        let mut state = self.state.write().await;
        state.event_handler = handler;
    }

    pub async fn disconnect(&self) -> Result<()> {
        let websocket_tx = self.websocket_tx.lock().await;
        if let Some(sender) = websocket_tx.as_ref() {
            let _ = sender.send(Message::Close(None));
        }

        let mut state = self.state.write().await;
        state.connected = false;
        state.pending_requests.clear();

        Ok(())
    }
}

async fn connect(client: &HarmonyClient, is_reconnect: bool) -> Result<()> {
    let url = Url::parse(&client.options.server_url)
        .map_err(|e| HarmonyError::InvalidInput(format!("Invalid server URL: {}", e)))?;

    let (ws_stream, _) = timeout(client.options.timeout, connect_async(url.to_string()))
        .await
        .map_err(|_| HarmonyError::Internal("Connection timeout".to_string()))?
        .map_err(HarmonyError::WebSocket)?;

    let (ws_tx, mut ws_rx) = ws_stream.split();
    let (sender, mut receiver) = mpsc::unbounded_channel();

    {
        let mut websocket_tx = client.websocket_tx.lock().await;
        *websocket_tx = Some(sender);
    }

    {
        let mut state = client.state.write().await;
        state.connected = true;
        state.reconnecting = false;
        state.reconnect_attempts = 0;
    }

    {
        let state = client.state.read().await;
        let handler = state.event_handler.clone();
        drop(state);

        if is_reconnect {
            handler.on_reconnected();
        } else {
            handler.on_connected();
        }
    }

    let ws_tx = Arc::new(Mutex::new(ws_tx));
    let ws_tx_clone = ws_tx.clone();
    tokio::spawn(async move {
        while let Some(message) = receiver.recv().await {
            let tx = ws_tx_clone.lock().await;
            if let Err(e) = tx.send(message).await {
                eprintln!("Failed to send WebSocket message: {}", e);
                break;
            }
        }
    });

    let state_clone = client.state.clone();
    let websocket_tx_clone = client.websocket_tx.clone();
    let reconnect_trigger = client.reconnect_tx.clone();

    tokio::spawn(async move {
        while let Some(message) = ws_rx.next().await {
            match message {
                Ok(Message::Binary(data)) => {
                    if let Err(e) = HarmonyClient::handle_binary_message(
                        state_clone.clone(),
                        &data.to_vec().as_slice(),
                    )
                    .await
                    {
                        eprintln!("Failed to handle message: {}", e);
                    }
                }
                Ok(Message::Close(_)) | Err(_) => {
                    HarmonyClient::mark_disconnected(
                        state_clone.clone(),
                        websocket_tx_clone.clone(),
                    )
                    .await;

                    let _ = reconnect_trigger.send(());
                    break;
                }
                _ => {}
            }
        }
    });

    client.authenticate().await?; // TODO:

    Ok(())
}

async fn authenticate(client: &HarmonyClient) -> Result<()> {
    let identify_request = RpcApiRequest::Identify {
        token: client.options.token.clone(),
        public_key: vec![], // TODO:
    };

    let mut buf = Vec::new();
    identify_request
        .serialize(&mut rmp_serde::Serializer::new(&mut buf).with_struct_map())
        .map_err(|e| {
            HarmonyError::Internal(format!("Authentication serialization error: {}", e))
        })?;

    let websocket_tx = client.websocket_tx.lock().await;
    if let Some(sender) = websocket_tx.as_ref() {
        sender
            .send(Message::binary(buf))
            .map_err(|_| HarmonyError::NotConnected)?;
    } else {
        return Err(HarmonyError::NotConnected);
    }

    Ok(())
}
