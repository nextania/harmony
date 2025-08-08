//! Harmony API client implementation

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use rmpv::Value;

use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};
use url::Url;
use uuid::Uuid;

use crate::error::{HarmonyError, Result};
use crate::events::{Event, EventHandler, NoOpEventHandler, RpcApiEvent};
use crate::models::*;

/// Configuration for the Harmony client
#[derive(Clone, Debug)]
pub struct ClientConfig {
    /// WebSocket server URL
    pub server_url: String,
    /// JWT authentication token
    pub token: String,
    /// Connection timeout
    pub timeout: Duration,
    /// Whether to automatically reconnect on connection loss
    pub auto_reconnect: bool,
    /// Maximum number of reconnection attempts
    pub max_reconnect_attempts: u32,
}

impl ClientConfig {
    /// Create a new client configuration
    pub fn new(server_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            server_url: server_url.into(),
            token: token.into(),
            timeout: Duration::from_secs(30),
            auto_reconnect: true,
            max_reconnect_attempts: 5,
        }
    }

    /// Set the connection timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set auto-reconnect behavior
    pub fn with_auto_reconnect(mut self, enabled: bool) -> Self {
        self.auto_reconnect = enabled;
        self
    }

    /// Set maximum reconnection attempts
    pub fn with_max_reconnect_attempts(mut self, attempts: u32) -> Self {
        self.max_reconnect_attempts = attempts;
        self
    }
}

/// RPC method request matching server's format
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
enum RpcApiRequest {
    #[serde(rename_all = "camelCase")]
    Identify {
        token: String,
        public_key: Vec<u8>,
    },
    #[serde(rename_all = "camelCase")]
    Message {
        id: String,
        method: String,
        data: Value,
    },
}

/// RPC method response
#[derive(Debug, Deserialize)]
struct RpcApiResponse {
    #[allow(dead_code)]
    id: Option<String>,
    response: Option<Value>,
}

/// Internal client state
struct ClientState {
    /// Pending RPC requests
    pending_requests: HashMap<String, mpsc::UnboundedSender<Value>>,
    /// Event handler
    event_handler: Arc<dyn EventHandler>,
    /// Connection status
    connected: bool,
    /// Whether currently reconnecting
    reconnecting: bool,
    /// Current reconnection attempt count
    reconnect_attempts: u32,
}

/// Harmony API client
pub struct HarmonyClient {
    config: ClientConfig,
    state: Arc<RwLock<ClientState>>,
    websocket_tx: Arc<Mutex<Option<mpsc::UnboundedSender<WsMessage>>>>,
    reconnect_tx: mpsc::UnboundedSender<()>,
}

impl HarmonyClient {
    /// Create a new Harmony client and establish connection
    pub async fn new(config: ClientConfig) -> Result<Self> {
        let (reconnect_tx, reconnect_rx) = mpsc::unbounded_channel();
        
        let client = Self {
            config: config.clone(),
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

        // Spawn reconnection handler
        client.spawn_reconnection_handler(reconnect_rx).await;
        
        client.connect().await?;
        Ok(client)
    }

    /// Create a new client with a custom event handler
    pub async fn new_with_handler(
        config: ClientConfig,
        event_handler: Arc<dyn EventHandler>,
    ) -> Result<Self> {
        let (reconnect_tx, reconnect_rx) = mpsc::unbounded_channel();
        
        let client = Self {
            config: config.clone(),
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

        // Spawn reconnection handler
        client.spawn_reconnection_handler(reconnect_rx).await;

        client.connect().await?;
        Ok(client)
    }

    /// Establish WebSocket connection
    async fn connect(&self) -> Result<()> {
        self.connect_internal(false).await
    }

    /// Internal connection method with reconnection flag
    async fn connect_internal(&self, is_reconnect: bool) -> Result<()> {
        let url = Url::parse(&self.config.server_url)
            .map_err(|e| HarmonyError::InvalidInput(format!("Invalid server URL: {}", e)))?;

        let (ws_stream, _) = timeout(self.config.timeout, connect_async(url))
            .await
            .map_err(|_| HarmonyError::Internal("Connection timeout".to_string()))?
            .map_err(HarmonyError::WebSocket)?;

        let (ws_tx, mut ws_rx) = ws_stream.split();
        let (sender, mut receiver) = mpsc::unbounded_channel();

        // Store the sender for outgoing messages
        {
            let mut websocket_tx = self.websocket_tx.lock().await;
            *websocket_tx = Some(sender);
        }

        // Update connection status
        {
            let mut state = self.state.write().await;
            state.connected = true;
            state.reconnecting = false;
            state.reconnect_attempts = 0;
        }

        // Notify event handler of connection
        {
            let state = self.state.read().await;
            let handler = state.event_handler.clone();
            drop(state);
            
            if is_reconnect {
                handler.on_reconnected();
                handler.on_event(&Event::Reconnected);
            } else {
                handler.on_connected();
                handler.on_event(&Event::Connected);
            }
        }

        // Spawn task to handle outgoing messages
        let ws_tx = Arc::new(Mutex::new(ws_tx));
        let ws_tx_clone = ws_tx.clone();
        tokio::spawn(async move {
            while let Some(message) = receiver.recv().await {
                let mut tx = ws_tx_clone.lock().await;
                if let Err(e) = tx.send(message).await {
                    eprintln!("Failed to send WebSocket message: {}", e);
                    break;
                }
            }
        });

        // Spawn task to handle incoming messages and connection drops
        let state_clone = self.state.clone();
        let websocket_tx_clone = self.websocket_tx.clone();
        let reconnect_trigger = self.reconnect_tx.clone();
        
        tokio::spawn(async move {
            while let Some(message) = ws_rx.next().await {
                match message {
                    Ok(WsMessage::Binary(data)) => {
                        if let Err(e) = Self::handle_binary_message(state_clone.clone(), &data).await {
                            eprintln!("Failed to handle message: {}", e);
                        }
                    }
                    Ok(WsMessage::Close(_)) | Err(_) => {
                        // Connection closed or error - mark as disconnected and trigger reconnection
                        Self::mark_disconnected(state_clone.clone(), websocket_tx_clone.clone()).await;
                        
                        // Trigger reconnection
                        let _ = reconnect_trigger.send(());
                        break;
                    }
                    _ => {}
                }
            }
        });

        // Send authentication
        self.authenticate().await?;

        Ok(())
    }

    /// Mark connection as disconnected and clean up state
    async fn mark_disconnected(
        state: Arc<RwLock<ClientState>>,
        websocket_tx: Arc<Mutex<Option<mpsc::UnboundedSender<WsMessage>>>>,
    ) {
        // Clear websocket sender
        {
            let mut websocket_tx = websocket_tx.lock().await;
            *websocket_tx = None;
        }

        // Update state and notify
        {
            let mut state = state.write().await;
            state.connected = false;
            
            // Notify event handler of disconnection
            let handler = state.event_handler.clone();
            tokio::spawn(async move {
                handler.on_disconnected();
                handler.on_event(&Event::Disconnected);
            });
            
            // Close all pending request channels
            for (_, sender) in state.pending_requests.drain() {
                drop(sender);
            }
        }
    }

    /// Mark disconnected and trigger reconnection (used during initial connection)
    async fn mark_disconnected_and_trigger_reconnect(
        state: Arc<RwLock<ClientState>>,
        websocket_tx: Arc<Mutex<Option<mpsc::UnboundedSender<WsMessage>>>>,
    ) {
        Self::mark_disconnected(state, websocket_tx).await;
        // Note: The reconnection trigger is handled by the main client instance
        // This method is used for connections created by the reconnection handler
    }

    /// Spawn the reconnection handler task
    async fn spawn_reconnection_handler(&self, mut reconnect_rx: mpsc::UnboundedReceiver<()>) {
        let state = self.state.clone();
        let websocket_tx = self.websocket_tx.clone();
        let config = self.config.clone();
        
        tokio::spawn(async move {
            while reconnect_rx.recv().await.is_some() {
                // Check if we should reconnect
                let should_reconnect = {
                    let mut state = state.write().await;
                    if !config.auto_reconnect || state.connected || state.reconnecting {
                        continue;
                    }
                    state.reconnecting = true;
                    state.reconnect_attempts = 0;
                    true
                };

                if !should_reconnect {
                    continue;
                }

                // Perform reconnection attempts
                let mut attempt = 1;
                let max_attempts = config.max_reconnect_attempts;
                
                while attempt <= max_attempts {
                    {
                        let mut state = state.write().await;
                        state.reconnect_attempts = attempt;
                    }

                    // Notify event handler of reconnection attempt
                    {
                        let state_read = state.read().await;
                        let handler = state_read.event_handler.clone();
                        drop(state_read);
                        
                        handler.on_reconnecting(attempt, max_attempts);
                        handler.on_event(&Event::Reconnecting { attempt, max_attempts });
                    }

                    // Calculate backoff delay: 2^(attempt-1) seconds, max 30 seconds
                    if attempt > 1 {
                        let delay = Duration::from_secs((2_u64.pow((attempt - 1).min(5))).min(30));
                        tokio::time::sleep(delay).await;
                    }

                    // Attempt to reconnect
                    match Self::attempt_connection(state.clone(), websocket_tx.clone(), config.clone()).await {
                        Ok(()) => {
                            // Reconnection successful
                            {
                                let mut state = state.write().await;
                                state.connected = true;
                                state.reconnecting = false;
                                state.reconnect_attempts = 0;
                            }

                            // Notify event handler
                            {
                                let state_read = state.read().await;
                                let handler = state_read.event_handler.clone();
                                drop(state_read);
                                
                                handler.on_reconnected();
                                handler.on_event(&Event::Reconnected);
                            }
                            break;
                        }
                        Err(e) => {
                            eprintln!("Reconnection attempt {} failed: {}", attempt, e);
                            attempt += 1;
                        }
                    }
                }

                // Check if all attempts failed
                {
                    let state_read = state.read().await;
                    if state_read.reconnect_attempts >= max_attempts {
                        // All reconnection attempts failed
                        {
                            let mut state = state.write().await;
                            state.reconnecting = false;
                        }

                        let handler = state_read.event_handler.clone();
                        drop(state_read);
                        
                        handler.on_reconnection_failed(max_attempts);
                        handler.on_event(&Event::ReconnectionFailed { attempts: max_attempts });
                    }
                }
            }
        });
    }

    /// Attempt connection (used by reconnection handler)
    async fn attempt_connection(
        state: Arc<RwLock<ClientState>>,
        websocket_tx: Arc<Mutex<Option<mpsc::UnboundedSender<WsMessage>>>>,
        config: ClientConfig,
    ) -> Result<()> {
        let url = Url::parse(&config.server_url)
            .map_err(|e| HarmonyError::InvalidInput(format!("Invalid server URL: {}", e)))?;

        let (ws_stream, _) = timeout(config.timeout, connect_async(url))
            .await
            .map_err(|_| HarmonyError::Internal("Connection timeout".to_string()))?
            .map_err(HarmonyError::WebSocket)?;

        let (ws_tx, mut ws_rx) = ws_stream.split();
        let (sender, mut receiver) = mpsc::unbounded_channel();

        // Store the sender for outgoing messages
        {
            let mut websocket_tx = websocket_tx.lock().await;
            *websocket_tx = Some(sender);
        }

        // Spawn task to handle outgoing messages
        let ws_tx = Arc::new(Mutex::new(ws_tx));
        let ws_tx_clone = ws_tx.clone();
        tokio::spawn(async move {
            while let Some(message) = receiver.recv().await {
                let mut tx = ws_tx_clone.lock().await;
                if let Err(e) = tx.send(message).await {
                    eprintln!("Failed to send WebSocket message: {}", e);
                    break;
                }
            }
        });

        // Spawn task to handle incoming messages
        let state_clone = state.clone();
        let websocket_tx_clone = websocket_tx.clone();
        tokio::spawn(async move {
            while let Some(message) = ws_rx.next().await {
                match message {
                    Ok(WsMessage::Binary(data)) => {
                        if let Err(e) = Self::handle_binary_message(state_clone.clone(), &data).await {
                            eprintln!("Failed to handle message: {}", e);
                        }
                    }
                    Ok(WsMessage::Close(_)) | Err(_) => {
                        // Connection closed or error - mark as disconnected and trigger reconnection
                        Self::mark_disconnected_and_trigger_reconnect(state_clone.clone(), websocket_tx_clone.clone()).await;
                        break;
                    }
                    _ => {}
                }
            }
        });

        // Send authentication
        Self::authenticate_simple(websocket_tx, config).await?;

        Ok(())
    }

    /// Simple authentication for reconnection
    async fn authenticate_simple(
        websocket_tx: Arc<Mutex<Option<mpsc::UnboundedSender<WsMessage>>>>,
        config: ClientConfig,
    ) -> Result<()> {
        let identify_request = RpcApiRequest::Identify {
            token: config.token,
            public_key: vec![],
        };

        let mut buf = Vec::new();
        identify_request.serialize(&mut rmp_serde::Serializer::new(&mut buf).with_struct_map())
            .map_err(|e| HarmonyError::Internal(format!("Authentication serialization error: {}", e)))?;

        let websocket_tx = websocket_tx.lock().await;
        if let Some(sender) = websocket_tx.as_ref() {
            sender.send(WsMessage::Binary(buf))
                .map_err(|_| HarmonyError::NotConnected)?;
        } else {
            return Err(HarmonyError::NotConnected);
        }

        Ok(())
    }

    /// Send authentication message
    async fn authenticate(&self) -> Result<()> {
        // Send Identify message with the token
        let identify_request = RpcApiRequest::Identify {
            token: self.config.token.clone(),
            public_key: vec![], // For now, use an empty public key
        };

        // Serialize to MessagePack
        let mut buf = Vec::new();
        identify_request.serialize(&mut rmp_serde::Serializer::new(&mut buf).with_struct_map())
            .map_err(|e| HarmonyError::Internal(format!("Authentication serialization error: {}", e)))?;

        // Send the authentication message
        let websocket_tx = self.websocket_tx.lock().await;
        if let Some(sender) = websocket_tx.as_ref() {
            sender.send(WsMessage::Binary(buf))
                .map_err(|_| HarmonyError::NotConnected)?;
        } else {
            return Err(HarmonyError::NotConnected);
        }

        Ok(())
    }

    /// Handle incoming binary WebSocket message
    async fn handle_binary_message(
        state: Arc<RwLock<ClientState>>,
        data: &[u8],
    ) -> Result<()> {
        // Deserialize the MessagePack data
        let mut deserializer = rmp_serde::Deserializer::new(&data[..]);
        let value: Value = match serde::Deserialize::deserialize(&mut deserializer) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Failed to deserialize MessagePack: {}", e);
                return Ok(());
            }
        };

        // First, check if this is a response to a pending request (has an ID field)
        if let Some(id_value) = value.as_map().and_then(|m| m.iter().find(|(k, _)| k.as_str() == Some("id"))) {
            if let Some(id) = id_value.1.as_str() {
                let mut state_lock = state.write().await;
                if let Some(sender) = state_lock.pending_requests.remove(id) {
                    // This is a response to a pending request - send it to the waiting task
                    let _ = sender.send(value);
                    return Ok(());
                }
                // If we get here, it means we received a response for a request we're not waiting for
                // This could happen if the request timed out but the server still sent a response
                // We'll just ignore it
                return Ok(());
            }
        }

        // If there's no ID field, this should be an event
        // Convert rmpv::Value to a serde-compatible format for event handling
        if let Ok(event_wrapper) = rmpv::ext::from_value::<RpcApiEvent>(value.clone()) {
            let state_lock = state.read().await;
            let handler = state_lock.event_handler.clone();
            drop(state_lock);

            // Handle the event
            handler.on_event(&event_wrapper.event);
            match &event_wrapper.event {
                Event::NewMessage(event) => handler.on_new_message(event),
                Event::RemoveFriend(user_id) => handler.on_remove_friend(user_id),
                Event::AddFriend(user_id) => handler.on_add_friend(user_id),
                Event::Connected => handler.on_connected(),
                Event::Disconnected => handler.on_disconnected(),
                Event::Reconnecting { attempt, max_attempts } => handler.on_reconnecting(*attempt, *max_attempts),
                Event::Reconnected => handler.on_reconnected(),
                Event::ReconnectionFailed { attempts } => handler.on_reconnection_failed(*attempts),
            }
        } else {
            // Unknown message format - log it for debugging but don't fail
            eprintln!("Received unknown message format: {:?}", value);
        }

        Ok(())
    }

    /// Send an RPC request and wait for response
    /// 
    /// This method properly handles concurrent requests by:
    /// 1. Assigning a unique ID to each request
    /// 2. Registering a channel to receive the specific response
    /// 3. Letting handle_binary_message route responses by ID to the correct waiting task
    /// 4. Cleaning up pending requests on timeout or connection issues
    async fn send_request<T, R>(&self, method: &str, params: T) -> Result<R>
    where
        T: Serialize,
        R: for<'de> Deserialize<'de>,
    {
        let request_id = Uuid::new_v4().to_string();
        
        // Convert params to rmpv::Value
        let params_value = rmpv::ext::to_value(params)
            .map_err(|e| HarmonyError::Internal(format!("Params serialization error: {}", e)))?;
        
        let request = RpcApiRequest::Message {
            id: request_id.clone(),
            method: method.to_string(),
            data: params_value,
        };

        // Serialize to MessagePack
        let mut buf = Vec::new();
        request.serialize(&mut rmp_serde::Serializer::new(&mut buf).with_struct_map())
            .map_err(|e| HarmonyError::Internal(format!("Serialization error: {}", e)))?;
        
        let (tx, mut rx) = mpsc::unbounded_channel();

        // Register pending request
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

        // Send the request
        {
            let websocket_tx = self.websocket_tx.lock().await;
            if let Some(sender) = websocket_tx.as_ref() {
                sender.send(WsMessage::Binary(buf))
                    .map_err(|_| HarmonyError::ConnectionLost)?;
            } else {
                return Err(HarmonyError::NotConnected);
            }
        }

        // Wait for response - our specific response will be sent to us by handle_binary_message
        let response_value = timeout(self.config.timeout, rx.recv())
            .await
            .map_err(|_| {
                // Clean up the pending request on timeout
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

    /// Check if the client is connected
    pub async fn is_connected(&self) -> bool {
        let state = self.state.read().await;
        state.connected
    }

    /// Check if the client is currently reconnecting
    pub async fn is_reconnecting(&self) -> bool {
        let state = self.state.read().await;
        state.reconnecting
    }

    /// Get current reconnection attempt count
    pub async fn reconnect_attempts(&self) -> u32 {
        let state = self.state.read().await;
        state.reconnect_attempts
    }

    /// Manually trigger reconnection (if not already connected)
    pub async fn reconnect(&self) -> Result<()> {
        let is_connected = {
            let state = self.state.read().await;
            state.connected
        };

        if is_connected {
            return Err(HarmonyError::Internal("Already connected".to_string()));
        }

        // Just try to connect again with the normal method
        self.connect_internal(true).await
    }

    /// Set event handler
    pub async fn set_event_handler(&self, handler: Arc<dyn EventHandler>) {
        let mut state = self.state.write().await;
        state.event_handler = handler;
    }

    /// Disconnect from the server
    pub async fn disconnect(&self) -> Result<()> {
        let websocket_tx = self.websocket_tx.lock().await;
        if let Some(sender) = websocket_tx.as_ref() {
            let _ = sender.send(WsMessage::Close(None));
        }

        let mut state = self.state.write().await;
        state.connected = false;
        state.pending_requests.clear();

        Ok(())
    }
}

// API method implementations
impl HarmonyClient {
    /// Get a specific channel by ID
    pub async fn get_channel(&self, channel_id: &str) -> Result<Channel> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            id: String,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            channel: Channel,
        }

        let response: Response = self
            .send_request(
                "GET_CHANNEL",
                Params {
                    id: channel_id.to_string(),
                },
            )
            .await?;

        Ok(response.channel)
    }

    /// Get all channels the user has access to
    pub async fn get_channels(&self) -> Result<Vec<Channel>> {
        #[derive(Serialize)]
        struct Params {}

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            channels: Vec<Channel>,
        }

        let response: Response = self.send_request("GET_CHANNELS", Params {}).await?;
        Ok(response.channels)
    }

    /// Get messages from a channel
    pub async fn get_messages(
        &self,
        channel_id: &str,
        limit: Option<i64>,
        latest: Option<bool>,
        before: Option<String>,
        after: Option<String>,
    ) -> Result<Vec<Message>> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            channel_id: String,
            limit: Option<i64>,
            latest: Option<bool>,
            before: Option<String>,
            after: Option<String>,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            messages: Vec<Message>,
        }

        let response: Response = self
            .send_request(
                "GET_MESSAGES",
                Params {
                    channel_id: channel_id.to_string(),
                    limit,
                    latest,
                    before,
                    after,
                },
            )
            .await?;

        Ok(response.messages)
    }

    /// Send a message to a channel
    pub async fn send_message(&self, channel_id: &str, content: &str) -> Result<Message> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            channel_id: String,
            content: String,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            message: Message,
        }

        let response: Response = self
            .send_request(
                "SEND_MESSAGE",
                Params {
                    channel_id: channel_id.to_string(),
                    content: content.to_string(),
                },
            )
            .await?;

        Ok(response.message)
    }

    /// Create an invite for a channel
    pub async fn create_invite(
        &self,
        channel_id: &str,
        max_uses: Option<i32>,
        expires_at: Option<u64>,
        authorized_users: Option<Vec<String>>,
    ) -> Result<Invite> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            channel_id: String,
            max_uses: Option<i32>,
            expires_at: Option<u64>,
            authorized_users: Option<Vec<String>>,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            invite: Invite,
        }

        let response: Response = self
            .send_request(
                "CREATE_INVITE",
                Params {
                    channel_id: channel_id.to_string(),
                    max_uses,
                    expires_at,
                    authorized_users,
                },
            )
            .await?;

        Ok(response.invite)
    }

    /// Delete an invite
    pub async fn delete_invite(&self, invite_id: &str) -> Result<()> {
        #[derive(Serialize)]
        struct Params {
            id: String,
        }

        #[derive(Deserialize)]
        struct Response {}

        let _: Response = self
            .send_request(
                "DELETE_INVITE",
                Params {
                    id: invite_id.to_string(),
                },
            )
            .await?;

        Ok(())
    }

    /// Get a specific invite by ID
    pub async fn get_invite(&self, invite_id: &str) -> Result<Invite> {
        #[derive(Serialize)]
        struct Params {
            id: String,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            invite: Invite,
        }

        let response: Response = self
            .send_request(
                "GET_INVITE",
                Params {
                    id: invite_id.to_string(),
                },
            )
            .await?;

        Ok(response.invite)
    }

    /// Get all invites for channels the user manages
    pub async fn get_invites(&self) -> Result<Vec<Invite>> {
        #[derive(Serialize)]
        struct Params {}

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            invites: Vec<Invite>,
        }

        let response: Response = self.send_request("GET_INVITES", Params {}).await?;
        Ok(response.invites)
    }

    /// Start a call in a channel
    pub async fn start_call(&self, channel_id: &str) -> Result<()> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            channel_id: String,
        }

        #[derive(Deserialize)]
        struct Response {}

        let _: Response = self
            .send_request(
                "START_CALL",
                Params {
                    channel_id: channel_id.to_string(),
                },
            )
            .await?;

        Ok(())
    }

    /// Join a call in a channel
    pub async fn join_call(&self, channel_id: &str, sdp: &str) -> Result<String> {
        #[derive(Serialize)]
        struct Params {
            id: String,
            sdp: String,
        }

        #[derive(Deserialize)]
        struct Response {
            sdp: String,
        }

        let response: Response = self
            .send_request(
                "JOIN_CALL",
                Params {
                    id: channel_id.to_string(),
                    sdp: sdp.to_string(),
                },
            )
            .await?;

        Ok(response.sdp)
    }

    /// Leave a call in a channel
    pub async fn leave_call(&self, channel_id: &str) -> Result<()> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            channel_id: String,
        }

        #[derive(Deserialize)]
        struct Response {}

        let _: Response = self
            .send_request(
                "LEAVE_CALL",
                Params {
                    channel_id: channel_id.to_string(),
                },
            )
            .await?;

        Ok(())
    }

    /// End a call in a channel
    pub async fn end_call(&self, channel_id: &str) -> Result<()> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            channel_id: String,
        }

        #[derive(Deserialize)]
        struct Response {}

        let _: Response = self
            .send_request(
                "END_CALL",
                Params {
                    channel_id: channel_id.to_string(),
                },
            )
            .await?;

        Ok(())
    }
}
