use std::{future::Future, pin::Pin, sync::Arc};

use async_tungstenite::{accept_async, tokio::TokioAdapter, tungstenite::Message};
use dashmap::DashMap;
use futures::{
    SinkExt, StreamExt,
    channel::mpsc::{UnboundedSender, unbounded},
    future::BoxFuture,
};
use rmp_serde::{Deserializer, Serializer};
use rmpv::{
    Value,
    ext::{from_value, to_value},
};
use serde::{Deserialize, Serialize};
use tokio::{
    net::{TcpListener, TcpStream},
    task,
    time::timeout,
};
use tracing::{Instrument, debug, info};
use uuid::Uuid;

use crate::{
    errors::Error,
    rate_limit::RateLimiter,
    utilities::{HEARTBEAT_TIMEOUT, generate_id},
};

const MAX_PRE_AUTH_MESSAGES: usize = 5;

#[cfg(feature = "otel")]
use opentelemetry::{
    KeyValue, global,
    metrics::{Counter, Histogram, UpDownCounter},
};
#[cfg(feature = "otel")]
use std::sync::OnceLock;

#[cfg(feature = "otel")]
fn meter() -> opentelemetry::metrics::Meter {
    global::meter("rapid")
}

#[cfg(feature = "otel")]
static RPC_CALLS: OnceLock<Counter<u64>> = OnceLock::new();
#[cfg(feature = "otel")]
static RPC_CONNECTIONS: OnceLock<UpDownCounter<i64>> = OnceLock::new();
#[cfg(feature = "otel")]
static RPC_DURATION_MS: OnceLock<Histogram<f64>> = OnceLock::new();
#[cfg(feature = "otel")]
static RPC_RATE_LIMITED: OnceLock<Counter<u64>> = OnceLock::new();

#[cfg(feature = "otel")]
fn rpc_calls() -> &'static Counter<u64> {
    RPC_CALLS.get_or_init(|| {
        meter()
            .u64_counter("rapid.rpc.calls")
            .with_description("Number of RPC method calls dispatched")
            .build()
    })
}

#[cfg(feature = "otel")]
fn rpc_connections() -> &'static UpDownCounter<i64> {
    RPC_CONNECTIONS.get_or_init(|| {
        meter()
            .i64_up_down_counter("rapid.connections.active")
            .with_description("Active WebSocket connections")
            .build()
    })
}

#[cfg(feature = "otel")]
fn rpc_duration_ms() -> &'static Histogram<f64> {
    RPC_DURATION_MS.get_or_init(|| {
        meter()
            .f64_histogram("rapid.rpc.duration_ms")
            .with_description("RPC method dispatch duration in milliseconds")
            .with_unit("ms")
            .build()
    })
}

#[cfg(feature = "otel")]
fn rpc_rate_limited() -> &'static Counter<u64> {
    RPC_RATE_LIMITED.get_or_init(|| {
        meter()
            .u64_counter("rapid.rpc.rate_limited")
            .with_description("Number of RPC calls rejected by the rate limiter")
            .build()
    })
}

#[derive(Clone, Debug)]
pub struct RpcClient {
    id: String,
    socket: UnboundedSender<Message>,
    user_id: Option<String>,
    heartbeat_tx: UnboundedSender<()>,
}

impl RpcClient {
    async fn send(&mut self, data: Vec<u8>) {
        self.socket
            .send(Message::Binary(data.into()))
            .await
            .expect("Failed to send message");
    }

    pub fn unique_id(&self) -> &str {
        &self.id
    }

    pub fn user_id(&self) -> Option<&str> {
        self.user_id.as_deref()
    }

    pub fn emit<T: Serialize + Send + Clone + 'static>(&self, data: T) {
        let bytes = serialize(&RpcMessageS2C::Event {
            event: to_value(&data).expect("Failed to serialize"),
        })
        .expect("Failed to serialize");
        self.emit_raw(bytes);
    }

    fn emit_raw(&self, bytes: Vec<u8>) {
        let mut socket = self.socket.clone();
        task::spawn(async move {
            socket
                .send(Message::Binary(bytes.into()))
                .await
                .expect("Failed to send message");
        });
    }
}

pub trait RpcResponder {
    fn into_value(self) -> Value;
}

pub struct RpcValue<T>(pub T);

impl<T: Serialize> RpcResponder for RpcValue<T> {
    fn into_value(self) -> Value {
        to_value(&self.0).unwrap()
    }
}
impl<T: RpcResponder, U: RpcResponder> RpcResponder for Result<T, U> {
    fn into_value(self) -> Value {
        match self {
            Ok(value) => value.into_value(),
            Err(error) => error.into_value(),
        }
    }
}

impl RpcResponder for () {
    fn into_value(self) -> Value {
        unreachable!()
    }
}

pub trait RpcRequest {
    fn from_value(value: Value) -> Result<Self, Error>
    where
        Self: Sized;
}

impl<T> RpcValue<T> {
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T: for<'a> Deserialize<'a>> RpcRequest for RpcValue<T> {
    fn from_value(value: Value) -> Result<Self, Error> {
        let val = from_value::<T>(value);
        match val {
            Ok(v) => Ok(RpcValue(v)),
            Err(e) => Err(e.into()),
        }
    }
}

pub type AuthenticateFn = Box<dyn CloneableAuthenticateFn>;
pub trait CloneableAuthenticateFn:
    Fn(String) -> BoxFuture<'static, Result<String, Error>> + Send + Sync
{
    fn clone_box<'a>(&self) -> Box<dyn 'a + CloneableAuthenticateFn>
    where
        Self: 'a;
}
impl<F> CloneableAuthenticateFn for F
where
    F: Fn(String) -> BoxFuture<'static, Result<String, Error>> + Clone + Send + Sync,
{
    fn clone_box<'a>(&self) -> Box<dyn 'a + CloneableAuthenticateFn>
    where
        Self: 'a,
    {
        Box::new(self.clone())
    }
}
impl<'a> Clone for Box<dyn 'a + CloneableAuthenticateFn> {
    fn clone(&self) -> Self {
        (**self).clone_box()
    }
}

pub trait MethodFn: Fn(RpcState, Value) -> BoxFuture<'static, Value> + Send + Sync {
    fn clone_box<'a>(&self) -> Box<dyn 'a + MethodFn>
    where
        Self: 'a;
}
impl<F> MethodFn for F
where
    F: Fn(RpcState, Value) -> BoxFuture<'static, Value> + Clone + Send + Sync,
{
    fn clone_box<'a>(&self) -> Box<dyn 'a + MethodFn>
    where
        Self: 'a,
    {
        Box::new(self.clone())
    }
}
impl<'a> Clone for Box<dyn 'a + MethodFn> {
    fn clone(&self) -> Self {
        (**self).clone_box()
    }
}

pub trait Handler<G>: Clone + 'static {
    type Output;
    type Future: Future<Output = Self::Output>;
    fn call(&self, state: RpcState, request: G) -> Self::Future;
}

impl<F, G, Fut> Handler<G> for F
where
    F: Fn(RpcState, G) -> Fut + Clone + 'static,
    Fut: Future,
{
    type Output = Fut::Output;
    type Future = Fut;
    fn call(&self, state: RpcState, request: G) -> Self::Future {
        self(state, request)
    }
}

#[derive(Clone, Debug)]
pub struct RpcClients(Arc<DashMap<String, RpcClient>>);

impl RpcClients {
    pub fn emit_all<T: Serialize + Send + Clone + 'static>(&self, data: T) {
        let bytes = serialize(&RpcMessageS2C::Event {
            event: to_value(&data).expect("Failed to serialize"),
        })
        .expect("Failed to serialize");
        for client in self.0.iter() {
            client.value().emit_raw(bytes.clone());
        }
    }

    pub fn emit_by<T: Serialize + Send + Clone + 'static, F: Fn(&RpcClient) -> bool>(
        &self,
        data: T,
        filter: F,
    ) {
        let bytes = serialize(&RpcMessageS2C::Event {
            event: to_value(&data).expect("Failed to serialize"),
        })
        .expect("Failed to serialize");
        for client in self.0.iter().filter(|c| filter(c.value())) {
            client.value().emit_raw(bytes.clone());
        }
    }
}

pub struct RpcState {
    clients: RpcClients,
    id: String,
}

impl RpcState {
    pub fn clients(&self) -> RpcClients {
        self.clients.clone()
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn client(&self) -> RpcClient {
        self.clients
            .0
            .get(&self.id)
            .map(|c| c.value().clone())
            .expect("Failed to get client")
    }

    pub fn is_authenticated(&self) -> bool {
        self.client().user_id.is_some()
    }

    pub fn user_id(&self) -> Option<String> {
        self.clients.0.get(&self.id).and_then(|c| c.user_id.clone())
    }
}

pub struct RpcServer {
    clients: RpcClients,
    authenticate: AuthenticateFn,
    methods: Arc<DashMap<String, Box<dyn MethodFn>>>,
    rate_limiter: Option<Arc<dyn RateLimiter>>,
}

impl RpcServer {
    pub fn new(authenticate: AuthenticateFn) -> Self {
        Self {
            clients: RpcClients(Arc::new(DashMap::new())),
            authenticate,
            methods: Arc::new(DashMap::new()),
            rate_limiter: None,
        }
    }

    pub fn rate_limiter(mut self, limiter: impl RateLimiter + 'static) -> Self {
        self.rate_limiter = Some(Arc::new(limiter));
        self
    }

    pub fn clients(&self) -> RpcClients {
        self.clients.clone()
    }

    pub fn register<F, G>(self, name: &str, method: F) -> Self
    where
        F: Handler<G> + Sync + Send,
        G: RpcRequest + Send,
        F::Output: RpcResponder + 'static,
        F::Future: Send + 'static,
    {
        info!("Registering method: {}", name);
        let x = Box::new(move |state: RpcState, val: Value| {
            let method = method.clone();
            let n: Pin<Box<dyn Future<Output = Value> + Send>> = Box::pin(async move {
                let g = G::from_value(val);
                let g = match g {
                    Ok(g) => g,
                    Err(e) => return RpcValue(e).into_value(),
                };
                let res = method.call(state, g).await;
                res.into_value()
            });
            n
        });
        self.methods.insert(name.to_string(), x);
        self
    }

    pub async fn start(&self, address: String) {
        let server = TcpListener::bind(address).await.unwrap();
        while let Ok((stream, _)) = server.accept().await {
            let clients = self.clients.clone();
            let fnc = self.authenticate.clone();
            let methods = self.methods.clone();
            let rate_limiter = self.rate_limiter.clone();
            task::spawn(
                async move { start_client(stream, clients, fnc, methods, rate_limiter).await },
            );
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RpcMessageC2S {
    #[serde(rename_all = "camelCase")]
    Identify {
        token: String,
    },
    Heartbeat {},
    Message {
        id: String,
        method: String,
        data: Value,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "type")]
pub enum RpcMessageS2C {
    #[serde(rename_all = "camelCase")]
    Hello {},
    Identify {},
    Heartbeat {},
    Error {
        error: Error,
    },
    Message {
        id: String,
        data: Value,
    },
    Event {
        event: Value,
    },
}

async fn start_client(
    connection: TcpStream,
    clients: RpcClients,
    authenticate: AuthenticateFn,
    methods: Arc<DashMap<String, Box<dyn MethodFn>>>,
    rate_limiter: Option<Arc<dyn RateLimiter>>,
) {
    info!("Socket connected: {}", connection.peer_addr().unwrap());
    #[cfg(feature = "otel")]
    rpc_connections().add(1, &[]);
    let ws_stream = accept_async(TokioAdapter::new(connection)).await;
    let Ok(ws_stream) = ws_stream else {
        return;
    };
    let (mut write, mut read) = ws_stream.split();
    let (mut s, mut r) = unbounded::<Message>();
    task::spawn(async move {
        while let Some(msg) = r.next().await {
            write.send(msg).await.expect("Failed to send message");
        }
        write.close(None).await.expect("Failed to close");
    });
    let id = generate_id();
    let val = RpcMessageS2C::Hello {};
    s.send(Message::Binary(
        serialize(&val).expect("Failed to serialize").into(),
    ))
    .await
    .expect("Failed to send message");

    let (tx, mut rx) = unbounded::<()>();
    let clients_moved = clients.clone();
    let id_moved = id.clone();
    task::spawn(async move {
        while timeout(
            std::time::Duration::from_millis(HEARTBEAT_TIMEOUT),
            rx.next(),
        )
        .await
        .is_ok()
        {}
        if let Some((_, mut client)) = clients_moved.0.remove(&id_moved) {
            client.socket.close().await.ok();
        }
    });
    let client = RpcClient {
        id: id.clone(),
        socket: s,
        user_id: None,
        heartbeat_tx: tx,
    };
    clients.0.insert(id.clone(), client);

    let mut is_authenticated = false;
    let mut pre_auth_count: usize = 0;

    while let Some(data) = read.next().await {
        let Ok(data) = data else {
            break;
        };
        match data {
            Message::Binary(bin) => {
                // flood protection - drop if too many messages are sent before successful authentication
                if !is_authenticated {
                    pre_auth_count += 1;
                    if pre_auth_count > MAX_PRE_AUTH_MESSAGES {
                        debug!(
                            "Pre-auth message limit exceeded, dropping connection {}",
                            id
                        );
                        if let Some((_, mut client)) = clients.0.remove(&id) {
                            client.socket.close().await.ok();
                        }
                        break;
                    }
                }

                let response = handle_packet(
                    bin.to_vec(),
                    &clients,
                    &id,
                    authenticate.clone(),
                    methods.clone(),
                    &rate_limiter,
                )
                .await;

                if matches!(&response, RpcMessageS2C::Identify {}) {
                    is_authenticated = true;
                }

                let serialized = serialize(&response).expect("Failed to serialize");
                debug!("Sent: {:?}", response);
                if let Some(mut client) = clients.0.get_mut(&id) {
                    client.send(serialized).await;
                } else {
                    debug!("Client {} disconnected before response could be sent", id);
                }
            }
            Message::Close(_) => {
                debug!("Received close");
            }
            _ => {
                debug!("Received unknown message");
                if let Some((_, mut client)) = clients.0.remove(&id.clone()) {
                    client.socket.close().await.ok();
                }
            }
        }
    }
    #[cfg(feature = "otel")]
    rpc_connections().add(-1, &[]);
    debug!("Connection {} closed", id);
}

pub async fn handle_packet(
    bin: Vec<u8>,
    clients: &RpcClients,
    user_id: &String,
    authenticate: AuthenticateFn,
    methods: Arc<DashMap<String, Box<dyn MethodFn>>>,
    rate_limiter: &Option<Arc<dyn RateLimiter>>,
) -> RpcMessageS2C {
    let result = deserialize::<RpcMessageC2S>(bin.as_slice());
    if let Ok(r) = result {
        debug!("Received: {:?}", r);
        match r {
            RpcMessageC2S::Identify { token } => authenticate(token.clone())
                .await
                .map(|uid| {
                    let mut client = clients.0.get_mut(user_id).unwrap();
                    client.user_id = Some(uid);
                    RpcMessageS2C::Identify {}
                })
                .unwrap_or_else(|e| RpcMessageS2C::Error { error: e }),
            RpcMessageC2S::Heartbeat {} => {
                let mut client = clients.0.get_mut(user_id).unwrap();
                client.heartbeat_tx.send(()).await.unwrap();
                RpcMessageS2C::Heartbeat {}
            }
            RpcMessageC2S::Message { id, method, data } => {
                // check if id is a uuid
                if Uuid::try_parse(&id).is_err() {
                    return RpcMessageS2C::Error {
                        error: Error::InvalidRequestId,
                    };
                }

                if let Some(rl) = rate_limiter {
                    let client_user_id = clients.0.get(user_id).and_then(|c| c.user_id.clone());
                    if let Some(uid) = client_user_id {
                        if !rl.check_rate_limit(&uid, &method).await {
                            #[cfg(feature = "otel")]
                            rpc_rate_limited().add(1, &[KeyValue::new("method", method.clone())]);
                            return RpcMessageS2C::Message {
                                id,
                                data: to_value(&Error::RateLimited).expect("Failed to serialize"),
                            };
                        }
                    }
                }

                let method_fn = methods.get(&method);
                let Some(method_fn) = method_fn else {
                    return RpcMessageS2C::Error {
                        error: Error::InvalidMethod,
                    };
                };
                #[cfg(feature = "otel")]
                let method_attrs = [KeyValue::new("method", method.clone())];
                #[cfg(feature = "otel")]
                rpc_calls().add(1, &method_attrs);
                #[cfg(feature = "otel")]
                let start = std::time::Instant::now();
                let span = tracing::info_span!("rpc.method", method = %method);
                let result = method_fn(
                    RpcState {
                        clients: clients.clone(),
                        id: user_id.clone(),
                    },
                    data,
                )
                .instrument(span)
                .await;
                #[cfg(feature = "otel")]
                rpc_duration_ms().record(start.elapsed().as_secs_f64() * 1000.0, &method_attrs);
                RpcMessageS2C::Message { id, data: result }
            }
        }
    } else {
        RpcMessageS2C::Error {
            error: Error::InvalidMethod,
        }
    }
}

pub fn serialize<T: Serialize>(value: &T) -> Result<Vec<u8>, rmp_serde::encode::Error> {
    let mut buf = Vec::new();
    value.serialize(&mut Serializer::new(&mut buf).with_struct_map())?;
    Ok(buf)
}

pub fn deserialize<T: for<'a> Deserialize<'a>>(buf: &[u8]) -> Result<T, rmp_serde::decode::Error> {
    let mut deserializer = Deserializer::new(buf);
    Deserialize::deserialize(&mut deserializer)
}
