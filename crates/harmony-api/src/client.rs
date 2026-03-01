use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use dashmap::DashMap;
use futures_util::StreamExt;
use rmpv::Value;
use serde::{Deserialize, Serialize};

use async_tungstenite::{tokio::connect_async, tungstenite::protocol::Message};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::time::timeout;
use url::Url;
use uuid::Uuid;

use crate::error::{ApiError, HarmonyError, Result};
use crate::events::{Event, RpcMessageS2C};

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
    evt_tx: mpsc::UnboundedSender<Event>,
    connected: Arc<AtomicBool>,
    reconnecting: Arc<AtomicBool>,
    reconnect_attempts: Arc<AtomicU32>,
    manually_disconnected: Arc<AtomicBool>,
    pending_requests: Arc<DashMap<String, oneshot::Sender<Value>>>,
    auth_request: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

impl HarmonyClient {
    pub async fn new_with_recv(
        options: ClientOptions,
        evt_tx: mpsc::UnboundedSender<Event>,
    ) -> Result<Self> {
        let (ws_tx, ws_rx) = mpsc::unbounded_channel();
        let (auth_tx, auth_rx) = oneshot::channel();

        let client = Self {
            options: options.clone(),
            connected: Arc::new(AtomicBool::new(false)),
            reconnecting: Arc::new(AtomicBool::new(false)),
            reconnect_attempts: Arc::new(AtomicU32::new(0)),
            websocket_tx: ws_tx,
            evt_tx,
            pending_requests: Arc::new(DashMap::new()),
            manually_disconnected: Arc::new(AtomicBool::new(false)),
            auth_request: Arc::new(Mutex::new(Some(auth_tx))),
        };

        client.connect(ws_rx, auth_rx).await?;
        Ok(client)
    }

    pub async fn new(options: ClientOptions) -> Result<(Self, mpsc::UnboundedReceiver<Event>)> {
        let (evt_tx, evt_rx) = mpsc::unbounded_channel();
        Ok((Self::new_with_recv(options, evt_tx).await?, evt_rx))
    }

    pub(crate) async fn send_request<T, R>(&self, method: &str, params: T) -> Result<R>
    where
        T: Serialize,
        R: for<'de> Deserialize<'de>,
    {
        let request_id = Uuid::new_v4().to_string();

        let params_value = rmpv::ext::to_value(params)
            .map_err(|e| HarmonyError::Internal(format!("Params serialization error: {}", e)))?;

        let request = RpcMessageC2S::Message {
            id: request_id.clone(),
            method: method.to_string(),
            data: params_value,
        };

        let mut buf = Vec::new();
        request
            .serialize(&mut rmp_serde::Serializer::new(&mut buf).with_struct_map())
            .map_err(|e| HarmonyError::Internal(format!("Serialization error: {}", e)))?;

        let (tx, rx) = oneshot::channel();

        if !self.connected.load(Ordering::SeqCst) {
            return Err(HarmonyError::NotConnected);
        }
        self.pending_requests.insert(request_id.clone(), tx);

        self.websocket_tx
            .send(Message::binary(buf))
            .map_err(|_| HarmonyError::ConnectionLost)?;
        let response_value = timeout(self.options.timeout, rx)
            .await
            .map_err(|_| {
                let pending_requests = self.pending_requests.clone();
                let request_id = request_id.clone();
                tokio::spawn(async move {
                    pending_requests.remove(&request_id);
                });
                HarmonyError::Internal("Request timeout".to_string())
            })?
            .or_else(|_| {
                Err(HarmonyError::Internal(
                    "Response channel closed".to_string(),
                ))
            })?;

        let result: R = rmpv::ext::from_value(response_value.clone()).map_err(|e| {
            let err_result: std::result::Result<ApiError, _> =
                rmpv::ext::from_value(response_value);
            match err_result {
                Ok(api_error) => HarmonyError::Api(api_error),
                Err(_) => HarmonyError::MessagePackExt(e),
            }
        })?;

        Ok(result)
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

    async fn connect(
        &self,
        mut receiver: UnboundedReceiver<Message>,
        auth_rx: oneshot::Receiver<()>,
    ) -> Result<()> {
        let url = Url::parse(&self.options.server_url)
            .map_err(|e| HarmonyError::InvalidInput(format!("Invalid server URL: {}", e)))?;

        let evt_tx = self.evt_tx.clone();
        let connected = self.connected.clone();
        let reconnecting = self.reconnecting.clone();
        let reconnect_attempts = self.reconnect_attempts.clone();
        let pending_requests = self.pending_requests.clone();
        let manually_disconnected = self.manually_disconnected.clone();
        let auth_request = self.auth_request.clone();
        let max_attempts = self.options.max_reconnect_attempts;
        let auto_reconnect = self.options.auto_reconnect;
        let timeout_duration = self.options.timeout;

        tokio::spawn(async move {
            let mut attempts = 0;
            loop {
                let result = timeout(timeout_duration, connect_async(url.to_string()))
                    .await
                    .map_err(|_| HarmonyError::Internal("Connection timeout".to_string()))
                    .and_then(|i| i.map_err(HarmonyError::WebSocket));
                match result {
                    Ok((stream, _)) => {
                        attempts = 0;
                        reconnect_attempts.store(0, Ordering::SeqCst);
                        connected.store(true, Ordering::SeqCst);
                        if reconnecting.swap(false, Ordering::SeqCst) {
                            evt_tx.send(Event::Reconnected).unwrap_or_else(|e| {
                                eprintln!("Failed to send reconnected event: {}", e);
                            });
                        } else {
                            evt_tx.send(Event::Connected).unwrap_or_else(|e| {
                                eprintln!("Failed to send connected event: {}", e);
                            });
                        }
                        let (mut ws_tx, mut ws_rx) = stream.split();
                        let mut next_heartbeat =
                            tokio::time::Instant::now() + Duration::from_secs(10);
                        loop {
                            tokio::select! {
                                Some(msg) = receiver.recv() => {
                                    if let Err(e) = ws_tx.send(msg).await {
                                        eprintln!("Failed to send WebSocket message: {}", e);
                                        break;
                                    }
                                }
                                Some(message) = ws_rx.next() => {
                                    match message {
                                        Ok(Message::Binary(data)) => {
                                            let mut deserializer = rmp_serde::Deserializer::new(data.as_ref());
                                            let value: Value = match serde::Deserialize::deserialize(&mut deserializer) {
                                                Ok(v) => v,
                                                Err(e) => {
                                                    eprintln!("Failed to deserialize MessagePack: {}", e);
                                                    break;
                                                }
                                            };

                                            if let Ok(event_wrapper) = rmpv::ext::from_value::<RpcMessageS2C>(value.clone()) {
                                                match event_wrapper {
                                                    RpcMessageS2C::Identify {} => {
                                                        println!("Authentication successful");
                                                        auth_request.lock().await.take().map(|tx| {
                                                            let _ = tx.send(());
                                                        });
                                                    }
                                                    RpcMessageS2C::Event { event } => {
                                                        if let Ok(event) = rmpv::ext::from_value::<Event>(event) {
                                                            evt_tx.send(event).unwrap_or_else(|e| {
                                                                eprintln!("Failed to send event: {}", e);
                                                            });
                                                        } else {
                                                            eprintln!("Failed to parse event: {:?}", value);
                                                            break;
                                                        }
                                                    }
                                                    RpcMessageS2C::Message { id, data } => {
                                                        if let Some((_, sender)) = pending_requests.remove(&id) {
                                                            let _ = sender.send(data);
                                                        } else {
                                                            eprintln!("Received response for unknown request ID: {}", id);
                                                        }
                                                    }
                                                    _ => {}
                                                }
                                            } else {
                                                eprintln!("Received unknown message format: {:?}", value);
                                            }
                                        }
                                        Ok(Message::Close(_)) | Err(_) => {
                                            connected.store(false, Ordering::SeqCst);
                                            eprintln!("Disconnected");
                                            break;
                                        }
                                        _ => {}
                                    }
                                }
                                _ = tokio::time::sleep_until(next_heartbeat) => {
                                    let mut buf = Vec::new();
                                    let mut serializer = rmp_serde::Serializer::new(&mut buf).with_struct_map();
                                    RpcMessageC2S::Heartbeat {}
                                        .serialize(&mut serializer)
                                        .expect("Failed to serialize heartbeat");
                                    if let Err(e) = ws_tx.send(Message::binary(buf)).await {
                                        eprintln!("Failed to send heartbeat: {}", e);
                                        break;
                                    }
                                    next_heartbeat = tokio::time::Instant::now() + Duration::from_secs(10);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        attempts += 1;
                        eprintln!("Connection attempt {} failed: {}", attempts, e);
                    }
                }
                evt_tx
                    .send(Event::Disconnected)
                    .unwrap_or_else(|e| eprintln!("Failed to send disconnected event: {}", e));
                if !auto_reconnect || manually_disconnected.load(Ordering::SeqCst) {
                    break;
                }
                reconnecting.store(true, Ordering::SeqCst);

                if attempts <= max_attempts {
                    reconnect_attempts.store(attempts, Ordering::SeqCst);
                    evt_tx
                        .send(Event::Reconnecting {
                            attempt: attempts,
                            max_attempts,
                        })
                        .unwrap_or_else(|e| {
                            eprintln!("Failed to send reconnecting event: {}", e);
                        });

                    // Calculate backoff delay: 2^(attempt-1) seconds, max 30 seconds
                    if attempts > 1 {
                        let delay = Duration::from_secs((2_u64.pow((attempts - 1).min(5))).min(30));
                        tokio::time::sleep(delay).await;
                    }
                } else {
                    reconnecting.store(false, Ordering::SeqCst);
                    evt_tx
                        .send(Event::ReconnectionFailed { attempts })
                        .unwrap_or_else(|e| {
                            eprintln!("Failed to send reconnect failed event: {}", e);
                        });
                }
            }
        });

        self.authenticate(auth_rx).await?;

        Ok(())
    }

    async fn authenticate(&self, mut rx: oneshot::Receiver<()>) -> Result<()> {
        let identify_request = RpcMessageC2S::Identify {
            token: self.options.token.clone(),
        };

        let mut buf = Vec::new();
        identify_request
            .serialize(&mut rmp_serde::Serializer::new(&mut buf).with_struct_map())
            .map_err(|e| {
                HarmonyError::Internal(format!("Authentication serialization error: {}", e))
            })?;

        self.websocket_tx
            .send(Message::binary(buf))
            .map_err(|_| HarmonyError::NotConnected)?;

        let auth_result = timeout(self.options.timeout, &mut rx)
            .await
            .map_err(|_| HarmonyError::Authentication("Authentication timed out".to_string()))
            .map(|r| {
                r.map_err(|_| HarmonyError::Authentication("Authentication failed".to_string()))
            })
            .flatten();
        if let Err(e) = auth_result {
            self.disconnect()?;
            return Err(e);
        }

        Ok(())
    }
}
