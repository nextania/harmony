use std::{any::Any, future::Future, pin::Pin, sync::Arc};

use async_tungstenite::{accept_async, tokio::TokioAdapter, tungstenite::Message};
use dashmap::DashMap;
use futures::{
    SinkExt, StreamExt,
    channel::mpsc::{UnboundedSender, unbounded},
    future::BoxFuture,
};
use log::{debug, info};
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
use uuid::Uuid;

use crate::{
    errors::Error,
    utilities::{HEARTBEAT_TIMEOUT, generate_id},
};

#[derive(Clone, Debug)]
pub struct RpcClient {
    id: String,
    socket: UnboundedSender<Message>,
    user: Option<Arc<Box<dyn Any + Send + Sync>>>,
    heartbeat_tx: UnboundedSender<()>,
}

impl RpcClient {
    async fn send(&mut self, data: Vec<u8>) {
        self.socket
            .send(Message::Binary(data.into()))
            .await
            .expect("Failed to send message");
    }

    pub fn get_user<T: 'static>(&self) -> Option<&T> {
        self.user.as_ref().and_then(|u| u.downcast_ref())
    }

    pub fn unique_id(&self) -> &str {
        &self.id
    }

    pub fn emit<T: Serialize + Send + Clone + 'static>(&self, data: T) {
        let mut socket = self.socket.clone();
        let data = data.clone();
        task::spawn(async move {
            socket
                .send(Message::Binary(
                    serialize(&RpcMessageS2C::Event {
                        event: to_value(&data).expect("Failed to serialize"),
                    })
                    .expect("Failed to serialize")
                    .into(),
                ))
                .await
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
    Fn(String) -> BoxFuture<'static, Result<Box<dyn Any + Send + Sync>, Error>> + Send + Sync
{
    fn clone_box<'a>(&self) -> Box<dyn 'a + CloneableAuthenticateFn>
    where
        Self: 'a;
}
impl<F> CloneableAuthenticateFn for F
where
    F: Fn(String) -> BoxFuture<'static, Result<Box<dyn Any + Send + Sync>, Error>>
        + Clone
        + Send
        + Sync,
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
        for client in self.0.iter() {
            client.value().emit(data.clone());
        }
    }

    pub fn emit_by<T: Serialize + Send + Clone + 'static, F: Fn(&RpcClient) -> bool>(
        &self,
        data: T,
        filter: F,
    ) {
        for client in self.0.iter().filter(|c| filter(c.value())) {
            client.value().emit(data.clone());
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
        self.client().user.is_some()
    }
}

pub struct RpcServer {
    clients: RpcClients,
    authenticate: AuthenticateFn,
    methods: Arc<DashMap<String, Box<dyn MethodFn>>>,
}

impl RpcServer {
    pub fn new(authenticate: AuthenticateFn) -> Self {
        Self {
            clients: RpcClients(Arc::new(DashMap::new())),
            authenticate,
            methods: Arc::new(DashMap::new()),
        }
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
            task::spawn(async move { start_client(stream, clients, fnc, methods).await });
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Serialize)]
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
) {
    println!("Socket connected: {}", connection.peer_addr().unwrap());
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
        user: None,
        heartbeat_tx: tx,
    };
    clients.0.insert(id.clone(), client);
    while let Some(data) = read.next().await {
        let Ok(data) = data else {
            break;
        };
        match data {
            Message::Binary(bin) => {
                let response = handle_packet(
                    bin.to_vec(),
                    &clients,
                    &id,
                    authenticate.clone(),
                    methods.clone(),
                )
                .await;
                let serialized = serialize(&response).expect("Failed to serialize");
                debug!("Sent: {:?}", response);
                let mut client = clients.0.get_mut(&id.clone()).unwrap();
                client.send(serialized).await;
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
}

pub async fn handle_packet(
    bin: Vec<u8>,
    clients: &RpcClients,
    user_id: &String,
    authenticate: AuthenticateFn,
    methods: Arc<DashMap<String, Box<dyn MethodFn>>>,
) -> RpcMessageS2C {
    let result = deserialize::<RpcMessageC2S>(bin.as_slice());
    if let Ok(r) = result {
        debug!("Received: {:?}", r);
        match r {
            RpcMessageC2S::Identify { token } => authenticate(token.clone())
                .await
                .map(|user| {
                    let mut client = clients.0.get_mut(user_id).unwrap();
                    client.user = Some(Arc::new(user));
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
                let method = methods.get(&method);
                let Some(method) = method else {
                    return RpcMessageS2C::Error {
                        error: Error::InvalidMethod,
                    };
                };
                let result = method(
                    RpcState {
                        clients: clients.clone(),
                        id: user_id.clone(),
                    },
                    data,
                )
                .await;
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
