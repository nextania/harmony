use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use core_api::Session;
use dashmap::DashMap;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_cbor_2::Value;

use async_tungstenite::{tokio::connect_async, tungstenite::protocol::Message};
use serde_cbor_2::value::{from_value, to_value};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::timeout;
use url::Url;
use uuid::Uuid;

use crate::error::{HarmonyError, Result};
use crate::events::{ClientEvent, Event, LifecycleEvent, RpcMessageS2C};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);

const EVENT_CHANNEL_CAPACITY: usize = 1024;

/// Configuration for the Harmony client
#[derive(Debug, Clone)]
pub struct ClientOptions {
    /// WebSocket server URL
    pub server_url: String,
    /// Connection timeout
    pub timeout: Duration,
    /// Whether to automatically reconnect on connection loss
    pub auto_reconnect: bool,
    /// Maximum number of reconnection attempts
    pub max_reconnect_attempts: u32,
}

impl ClientOptions {
    pub fn new(server_url: impl Into<String>) -> Self {
        Self {
            server_url: server_url.into(),
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
    identity: Arc<Session>,
}

type RpcResponse = std::result::Result<Value, Value>;

fn emit(evt_tx: &broadcast::Sender<ClientEvent>, event: impl Into<ClientEvent>) {
    if evt_tx.send(event.into()).is_err() {
        tracing::debug!("no event receivers; dropping event");
    }
}

impl HarmonyClient {
    pub async fn new_with_recv(
        identity: Arc<Session>,
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
            identity,
        };

        client.connect(ws_rx).await?;
        Ok(client)
    }

    pub async fn new(
        identity: Arc<Session>,
        options: ClientOptions,
    ) -> Result<(Self, broadcast::Receiver<ClientEvent>)> {
        let (evt_tx, evt_rx) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Ok((
            Self::new_with_recv(identity, options, evt_tx).await?,
            evt_rx,
        ))
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
            to_value(&params).map_err(|e| HarmonyError::Serialization(Box::new(e)))?;

        let request = RpcMessageC2S::Message {
            id: request_id.clone(),
            method: method.to_string(),
            data: params_value,
        };

        let buf =
            serde_cbor_2::to_vec(&request).map_err(|e| HarmonyError::Serialization(Box::new(e)))?;

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
            Ok(value) => from_value(value).map_err(|e| HarmonyError::Serialization(Box::new(e))),
            Err(error_value) => match from_value(error_value) {
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
        self.websocket_tx.send(Message::Close(None)).ok();

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
            max_attempts: self.options.max_reconnect_attempts,
            auto_reconnect: self.options.auto_reconnect,
            timeout_duration: self.options.timeout,
            first_outcome: first_tx,
            identity: self.identity.clone(),
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
            max_attempts,
            auto_reconnect,
            timeout_duration,
            first_outcome,
            identity,
        } = task;

        let mut first_outcome = Some(first_outcome);
        let mut attempts: u32 = 0;

        loop {
            let mut break_after = false;
            let mut authed_this_session = false;

            'establish: {
                // 1. get token
                let token = match identity.get_token().await {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::error!("failed to obtain auth token: {}", e);
                        if let Some(tx) = first_outcome.take() {
                            tx.send(Err(HarmonyError::Core(e))).ok();
                            break_after = true;
                        }
                        break 'establish;
                    }
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
                            tx.send(Err(e)).ok();
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
                    serde_cbor_2::to_vec(&msg).unwrap()
                };
                if let Err(e) = ws_tx.send(Message::binary(identify)).await {
                    tracing::warn!("failed to send identify: {}", e);
                    if let Some(tx) = first_outcome.take() {
                        tx.send(Err(HarmonyError::NotConnected)).ok();
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
                                    tracing::debug!("{:?}", data);
                                    let value: Value = match serde_cbor_2::from_slice(data.as_ref()) {
                                        Ok(v) => v,
                                        Err(e) => {
                                            tracing::warn!("failed to deserialize frame: {}", e);
                                            break;
                                        }
                                    };
                                    match from_value(value) {
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
                                                    tx.send(Ok(())).ok();
                                                }
                                            }
                                        }
                                        Ok(RpcMessageS2C::Event { event }) => {
                                            match from_value::<Event>(event) {
                                                Ok(event) => emit(&evt_tx, event),
                                                Err(e) => {
                                                    tracing::warn!("failed to parse event: {}", e);
                                                }
                                            }
                                        }
                                        Ok(RpcMessageS2C::Message { id, ok, data }) => {
                                            if let Some((_, sender)) = pending_requests.remove(&id) {
                                                sender.send(if ok { Ok(data) } else { Err(data) }).ok();
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
                            let heartbeat = RpcMessageC2S::Heartbeat {};
                            let Ok(buf) = serde_cbor_2::to_vec(&heartbeat) else {
                                tracing::error!("failed to serialize heartbeat");
                                break;
                            };
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
                    tx.send(Err(HarmonyError::AuthenticationClosed)).ok();
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
    max_attempts: u32,
    auto_reconnect: bool,
    timeout_duration: Duration,
    first_outcome: oneshot::Sender<Result<()>>,
    identity: Arc<Session>,
}
