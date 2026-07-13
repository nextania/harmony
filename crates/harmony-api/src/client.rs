use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use ciborium::value::Value;
use dashmap::DashMap;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use async_tungstenite::{tokio::connect_async, tungstenite::protocol::Message};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::timeout;
use url::Url;
use uuid::Uuid;

use crate::error::{ApiError, HarmonyError, Result};
use crate::events::{ClientEvent, Event, LifecycleEvent, RpcMessageS2C};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);

const EVENT_CHANNEL_CAPACITY: usize = 1024;

// TODO: finish designing account refresh system?
/// Supplies a fresh authentication token whenever the client authenticates.
pub trait TokenProvider: Send + Sync {
    fn fetch_token(&self) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>>;
}

/// Configuration for the Harmony client
#[derive(Clone)]
pub struct ClientOptions {
    /// WebSocket server URL
    pub server_url: String,
    /// Authentication token issued by AS. Used as the token when no
    /// [`TokenProvider`] is configured, and as the initial fallback otherwise.
    pub token: String,
    /// Connection timeout
    pub timeout: Duration,
    /// Whether to automatically reconnect on connection loss
    pub auto_reconnect: bool,
    /// Maximum number of reconnection attempts
    pub max_reconnect_attempts: u32,
    /// Optional hook to obtain a fresh token on each authentication.
    pub token_provider: Option<Arc<dyn TokenProvider>>,
}

impl std::fmt::Debug for ClientOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientOptions")
            .field("server_url", &self.server_url)
            .field("token", &"<redacted>")
            .field("timeout", &self.timeout)
            .field("auto_reconnect", &self.auto_reconnect)
            .field("max_reconnect_attempts", &self.max_reconnect_attempts)
            .field("token_provider", &self.token_provider.is_some())
            .finish()
    }
}

impl ClientOptions {
    pub fn new(server_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            server_url: server_url.into(),
            token: token.into(),
            timeout: Duration::from_secs(30),
            auto_reconnect: true,
            max_reconnect_attempts: 5,
            token_provider: None,
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

    pub fn with_token_provider(mut self, provider: Arc<dyn TokenProvider>) -> Self {
        self.token_provider = Some(provider);
        self
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
enum RpcMessageC2S {
    #[serde(rename_all = "camelCase")]
    Identify {
        token: String,
    },
    #[serde(rename_all = "camelCase")]
    Message {
        id: String,
        method: String,
        data: Value,
    },
    Heartbeat {},
}

#[derive(Clone)]
pub struct HarmonyClient {
    options: ClientOptions,
    websocket_tx: mpsc::UnboundedSender<Message>,
    evt_tx: broadcast::Sender<ClientEvent>,
    connected: Arc<AtomicBool>,
    reconnecting: Arc<AtomicBool>,
    reconnect_attempts: Arc<AtomicU32>,
    manually_disconnected: Arc<AtomicBool>,
    pending_requests: Arc<DashMap<String, oneshot::Sender<RpcResponse>>>,
}

type RpcResponse = std::result::Result<Value, Value>;

fn emit(evt_tx: &broadcast::Sender<ClientEvent>, event: impl Into<ClientEvent>) {
    if evt_tx.send(event.into()).is_err() {
        tracing::debug!("no event receivers; dropping event");
    }
}

impl HarmonyClient {
    pub async fn new_with_recv(
        options: ClientOptions,
        evt_tx: broadcast::Sender<ClientEvent>,
    ) -> Result<Self> {
        let (ws_tx, ws_rx) = mpsc::unbounded_channel();

        let client = Self {
            options: options.clone(),
            connected: Arc::new(AtomicBool::new(false)),
            reconnecting: Arc::new(AtomicBool::new(false)),
            reconnect_attempts: Arc::new(AtomicU32::new(0)),
            websocket_tx: ws_tx,
            evt_tx,
            pending_requests: Arc::new(DashMap::new()),
            manually_disconnected: Arc::new(AtomicBool::new(false)),
        };

        client.connect(ws_rx).await?;
        Ok(client)
    }

    pub async fn new(options: ClientOptions) -> Result<(Self, broadcast::Receiver<ClientEvent>)> {
        let (evt_tx, evt_rx) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Ok((Self::new_with_recv(options, evt_tx).await?, evt_rx))
    }

    /// Subscribe an additional consumer to the event stream. Each receiver
    /// gets every event emitted after the point of subscription.
    pub fn subscribe_events(&self) -> broadcast::Receiver<ClientEvent> {
        self.evt_tx.subscribe()
    }

    pub(crate) async fn send_request<T, R>(&self, method: &str, params: T) -> Result<R>
    where
        T: Serialize,
        R: for<'de> Deserialize<'de>,
    {
        let request_id = Uuid::new_v4().to_string();

        let params_value =
            Value::serialized(&params).map_err(|e| HarmonyError::Serialization(Box::new(e)))?;

        let request = RpcMessageC2S::Message {
            id: request_id.clone(),
            method: method.to_string(),
            data: params_value,
        };

        let mut buf = Vec::new();
        ciborium::into_writer(&request, &mut buf)
            .map_err(|e| HarmonyError::Serialization(Box::new(e)))?;

        let (tx, rx) = oneshot::channel();

        if !self.connected.load(Ordering::SeqCst) {
            return Err(HarmonyError::NotConnected);
        }
        self.pending_requests.insert(request_id.clone(), tx);

        self.websocket_tx
            .send(Message::binary(buf))
            .map_err(|_| HarmonyError::ConnectionLost)?;
        let response = timeout(self.options.timeout, rx)
            .await
            .map_err(|_| {
                self.pending_requests.remove(&request_id);
                HarmonyError::Timeout
            })?
            .map_err(|_| HarmonyError::ConnectionLost)?;

        match response {
            Ok(value) => value
                .deserialized::<R>()
                .map_err(|e| HarmonyError::Serialization(Box::new(e))),
            Err(error_value) => match error_value.deserialized::<ApiError>() {
                Ok(api_error) => Err(HarmonyError::Api(api_error)),
                Err(e) => Err(HarmonyError::Serialization(Box::new(e))),
            },
        }
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    pub fn is_reconnecting(&self) -> bool {
        self.reconnecting.load(Ordering::SeqCst)
    }

    pub fn reconnect_attempts(&self) -> u32 {
        self.reconnect_attempts.load(Ordering::SeqCst)
    }

    pub fn disconnect(&self) -> Result<()> {
        self.manually_disconnected.store(true, Ordering::SeqCst);
        let _ = self.websocket_tx.send(Message::Close(None));

        Ok(())
    }

    async fn connect(&self, receiver: UnboundedReceiver<Message>) -> Result<()> {
        let url = Url::parse(&self.options.server_url)
            .map_err(|e| HarmonyError::InvalidServerUrl(Box::new(e)))?;

        let (first_tx, first_rx) = oneshot::channel::<Result<()>>();

        tokio::spawn(Self::run_connection_task(ConnectionTask {
            url,
            receiver,
            evt_tx: self.evt_tx.clone(),
            connected: self.connected.clone(),
            reconnecting: self.reconnecting.clone(),
            reconnect_attempts: self.reconnect_attempts.clone(),
            pending_requests: self.pending_requests.clone(),
            manually_disconnected: self.manually_disconnected.clone(),
            static_token: self.options.token.clone(),
            token_provider: self.options.token_provider.clone(),
            max_attempts: self.options.max_reconnect_attempts,
            auto_reconnect: self.options.auto_reconnect,
            timeout_duration: self.options.timeout,
            first_outcome: first_tx,
        }));

        match first_rx.await {
            Ok(result) => result,
            Err(_) => Err(HarmonyError::ConnectionLost),
        }
    }

    async fn run_connection_task(task: ConnectionTask) {
        let ConnectionTask {
            url,
            mut receiver,
            evt_tx,
            connected,
            reconnecting,
            reconnect_attempts,
            pending_requests,
            manually_disconnected,
            static_token,
            token_provider,
            max_attempts,
            auto_reconnect,
            timeout_duration,
            first_outcome,
        } = task;

        let mut first_outcome = Some(first_outcome);
        let mut attempts: u32 = 0;

        loop {
            let mut break_after = false;
            let mut authed_this_session = false;

            'establish: {
                // 1. get token
                let token = if let Some(provider) = &token_provider {
                    match provider.fetch_token().await {
                        Ok(t) => t,
                        Err(e) => {
                            tracing::error!("failed to obtain auth token: {}", e);
                            if let Some(tx) = first_outcome.take() {
                                let _ = tx.send(Err(e));
                                break_after = true;
                            }
                            break 'establish;
                        }
                    }
                } else {
                    static_token.clone()
                };

                // 2. connect socket
                let conn = timeout(timeout_duration, connect_async(url.to_string()))
                    .await
                    .map_err(|_| HarmonyError::Timeout)
                    .and_then(|i| i.map_err(|e| HarmonyError::WebSocket(Box::new(e))));
                let (mut ws_tx, mut ws_rx) = match conn {
                    Ok((stream, _)) => stream.split(),
                    Err(e) => {
                        tracing::warn!("connection attempt failed: {}", e);
                        if let Some(tx) = first_outcome.take() {
                            let _ = tx.send(Err(e));
                            break_after = true;
                        }
                        break 'establish;
                    }
                };

                // 3. authenticate
                let identify = {
                    let msg = RpcMessageC2S::Identify {
                        token: token.clone(),
                    };
                    let mut buf = Vec::new();
                    if ciborium::into_writer(&msg, &mut buf).is_err() {
                        tracing::error!("failed to serialize identify");
                    }
                    buf
                };
                if let Err(e) = ws_tx.send(Message::binary(identify)).await {
                    tracing::warn!("failed to send identify: {}", e);
                    if let Some(tx) = first_outcome.take() {
                        let _ = tx.send(Err(HarmonyError::NotConnected));
                        break_after = true;
                    }
                    break 'establish;
                }

                // 4. run session loop
                let mut authed = false;
                let auth_deadline = tokio::time::Instant::now() + timeout_duration;
                let mut next_heartbeat = tokio::time::Instant::now() + HEARTBEAT_INTERVAL;

                loop {
                    tokio::select! {
                        msg = receiver.recv() => {
                            let Some(msg) = msg else {
                                // client has been dropped
                                break_after = true;
                                break;
                            };
                            if let Err(e) = ws_tx.send(msg).await {
                                tracing::warn!("failed to send message: {}", e);
                                break;
                            }
                        }
                        Some(message) = ws_rx.next() => {
                            match message {
                                Ok(Message::Binary(data)) => {
                                    let value: Value = match ciborium::from_reader(data.as_ref()) {
                                        Ok(v) => v,
                                        Err(e) => {
                                            tracing::warn!("failed to deserialize frame: {}", e);
                                            break;
                                        }
                                    };
                                    match value.deserialized::<RpcMessageS2C>() {
                                        Ok(RpcMessageS2C::Identify {}) => {
                                            if !authed {
                                                authed = true;
                                                authed_this_session = true;
                                                attempts = 0;
                                                reconnect_attempts.store(0, Ordering::SeqCst);
                                                connected.store(true, Ordering::SeqCst);
                                                let was_reconnecting =
                                                    reconnecting.swap(false, Ordering::SeqCst);
                                                tracing::debug!("authentication successful");
                                                if was_reconnecting {
                                                    emit(&evt_tx, LifecycleEvent::Reconnected);
                                                } else {
                                                    emit(&evt_tx, LifecycleEvent::Connected);
                                                }
                                                if let Some(tx) = first_outcome.take() {
                                                    let _ = tx.send(Ok(()));
                                                }
                                            }
                                        }
                                        Ok(RpcMessageS2C::Event { event }) => {
                                            match event.deserialized::<Event>() {
                                                Ok(event) => emit(&evt_tx, event),
                                                Err(e) => {
                                                    tracing::warn!("failed to parse event: {}", e);
                                                }
                                            }
                                        }
                                        Ok(RpcMessageS2C::Message { id, ok, data }) => {
                                            if let Some((_, sender)) = pending_requests.remove(&id) {
                                                let _ =
                                                    sender.send(if ok { Ok(data) } else { Err(data) });
                                            } else {
                                                tracing::debug!(
                                                    "response for unknown request id: {}",
                                                    id
                                                );
                                            }
                                        }
                                        Ok(_) => {}
                                        Err(_) => {
                                            tracing::warn!("unknown message format");
                                        }
                                    }
                                }
                                Ok(Message::Close(_)) | Err(_) => {
                                    tracing::debug!("connection closed");
                                    break;
                                }
                                _ => {}
                            }
                        }
                        _ = tokio::time::sleep_until(next_heartbeat) => {
                            let mut buf = Vec::new();
                            let heartbeat = RpcMessageC2S::Heartbeat {};
                            if ciborium::into_writer(&heartbeat, &mut buf).is_err() {
                                tracing::error!("failed to serialize heartbeat");
                                break;
                            }
                            if let Err(e) = ws_tx.send(Message::binary(buf)).await {
                                tracing::warn!("failed to send heartbeat: {}", e);
                                break;
                            }
                            next_heartbeat = tokio::time::Instant::now() + HEARTBEAT_INTERVAL;
                        }
                        _ = tokio::time::sleep_until(auth_deadline), if !authed => {
                            tracing::warn!("authentication timed out");
                            break;
                        }
                    }
                }

                connected.store(false, Ordering::SeqCst);
                pending_requests.clear();
                if authed_this_session {
                    emit(&evt_tx, LifecycleEvent::Disconnected);
                } else if let Some(tx) = first_outcome.take() {
                    let _ = tx.send(Err(HarmonyError::AuthenticationClosed));
                    break_after = true;
                }
            }

            if break_after {
                break;
            }
            if !auto_reconnect || manually_disconnected.load(Ordering::SeqCst) {
                break;
            }

            attempts = attempts.saturating_add(1);
            if attempts > max_attempts {
                reconnecting.store(false, Ordering::SeqCst);
                emit(&evt_tx, LifecycleEvent::ReconnectionFailed { attempts });
                break;
            }

            reconnecting.store(true, Ordering::SeqCst);
            reconnect_attempts.store(attempts, Ordering::SeqCst);
            emit(
                &evt_tx,
                LifecycleEvent::Reconnecting {
                    attempt: attempts,
                    max_attempts,
                },
            );

            // exponential backoff (2^(attempt-1)s, capped at 30s)
            if attempts > 1 {
                let delay = Duration::from_secs(2_u64.pow((attempts - 1).min(5)).min(30));
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    msg = receiver.recv() => {
                        if msg.is_none() {
                            // this client has been dropped
                            break;
                        }
                        // dropping this is safe because send_request does not send
                        // when not connected
                    }
                }
            }
        }
    }
}

struct ConnectionTask {
    url: Url,
    receiver: UnboundedReceiver<Message>,
    evt_tx: broadcast::Sender<ClientEvent>,
    connected: Arc<AtomicBool>,
    reconnecting: Arc<AtomicBool>,
    reconnect_attempts: Arc<AtomicU32>,
    pending_requests: Arc<DashMap<String, oneshot::Sender<RpcResponse>>>,
    manually_disconnected: Arc<AtomicBool>,
    static_token: String,
    token_provider: Option<Arc<dyn TokenProvider>>,
    max_attempts: u32,
    auto_reconnect: bool,
    timeout_duration: Duration,
    first_outcome: oneshot::Sender<Result<()>>,
}
