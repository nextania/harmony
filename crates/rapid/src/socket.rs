use std::{any::Any, future::Future, pin::Pin, sync::Arc};

use async_std::{
    channel::{unbounded, Sender},
    future,
    net::{TcpListener, TcpStream},
    task::spawn,
};
use async_tungstenite::{accept_async, tungstenite::Message};
use dashmap::DashMap;
use futures_util::{future::BoxFuture, SinkExt, StreamExt};
use log::debug;
use rand::rngs::OsRng;
use rmp_serde::{Deserializer, Serializer};
use rmpv::{ext::{from_value, to_value}, Value};
use serde::{Deserialize, Serialize};
use x25519_dalek::{EphemeralSecret, PublicKey};

use crate::{errors::Error, utilities::{generate_id, HEARTBEAT_TIMEOUT}};

#[derive(Clone)]
pub struct RpcClient {
    pub id: String,
    pub socket: Arc<Sender<Message>>,
    pub user: Option<Arc<Box<dyn Any + Send + Sync>>>,
    pub request_ids: Vec<String>,
    pub heartbeat_tx: Arc<Sender<()>>,
}

impl RpcClient {
    pub async fn send(&self, data: Vec<u8>) {                
        self
            .socket
            .send(Message::Binary(
                data,
            ))
            .await
            .expect("Failed to send message");
    }
    pub fn get_user<T: 'static>(&self) -> Option<&T> {
        self.user.as_ref().and_then(|u| u.downcast_ref())
    }
}

// pub type RpcMethod<T: RpcRequest> = dyn Fn(Arc<DashMap<String, RpcClient>>, String, T) -> impl RpcResponder;

pub trait RpcResponder {
    fn into_value(&self) -> Value;
}

pub struct RpcValue<T>(pub T);

impl<T: Serialize> RpcResponder for RpcValue<T> {
    fn into_value(&self) -> Value {
        to_value(&self.0).unwrap()
    }
}
impl<T: RpcResponder, U: RpcResponder> RpcResponder for Result<T, U> {
    fn into_value(&self) -> Value {
        match self {
            Ok(value) => value.into_value(),
            Err(error) => error.into_value(),
        }
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
pub trait CloneableAuthenticateFn: Fn(String) -> BoxFuture<'static, Result<Box<dyn Any + Send + Sync>, Error>> + Send + Sync {
    fn clone_box<'a>(&self) -> Box<dyn 'a + CloneableAuthenticateFn>
    where
        Self: 'a;
}
impl<F> CloneableAuthenticateFn for F
where
    F: Fn(String) -> BoxFuture<'static, Result<Box<dyn Any + Send + Sync>, Error>> + Clone + Send + Sync,
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




pub trait MethodFn: Fn(Arc<DashMap<String, RpcClient>>, String, Value) -> BoxFuture<'static, Value> + Send + Sync {
    fn clone_box<'a>(&self) -> Box<dyn 'a + MethodFn>
    where
        Self: 'a;
}
impl<F> MethodFn for F
where
    F: Fn(Arc<DashMap<String, RpcClient>>, String, Value) -> BoxFuture<'static, Value> + Clone + Send + Sync,
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
    fn call (&self, clients: Arc<DashMap<String, RpcClient>>, name: String, request: G) -> Self::Future;
}

impl<F, G, Fut> Handler<G> for F
where
    F: Fn(Arc<DashMap<String, RpcClient>>, String, G) -> Fut + Clone + 'static,
    Fut: Future,
{
    type Output = Fut::Output;
    type Future = Fut;
    fn call(&self, clients: Arc<DashMap<String, RpcClient>>, name: String, request: G) -> Self::Future {
        self(clients, name, request)
    }
}


pub struct RpcServer {
    clients: Arc<DashMap<String, RpcClient>>,
    authenticate: AuthenticateFn,
    methods: Arc<DashMap<String, Box<dyn MethodFn>>>,
}

impl RpcServer {
    pub fn new(authenticate: AuthenticateFn) -> Self {
        Self {
            clients: Arc::new(DashMap::new()),
            authenticate,
            methods: Arc::new(DashMap::new()),
        }
    }

    pub fn register<F, G>(self, name: &str, method: F) -> Self where 
        F: Handler<G> + Sync + Send,
        G: RpcRequest + Send,
        F::Output: RpcResponder + 'static,
        F::Future: Send + 'static,
    {
        let x = Box::new(move |clients: Arc<DashMap<String, RpcClient>>, id: String, val: Value| {
            let method = method.clone();
            let n: Pin<Box<dyn Future<Output = Value> + Send>> = Box::pin(async move {
                let g = G::from_value(val);
                let g = match g {
                    Ok(g) => g,
                    Err(e) => return RpcValue(e).into_value(),
                };
                let res = method.call(clients, id, g).await;
                res.into_value()
            });
            n
        });
        self.methods.insert(name.to_string(), x);
        self
    }

    pub async fn start(&self, address: String) {    
        let server = TcpListener::bind(address).await.unwrap();
        let mut incoming = server.incoming();
        while let Some(stream) = incoming.next().await {
            let clients = self.clients.clone();
            let fnc = self.authenticate.clone();
            let methods = self.methods.clone();
            spawn(async move { start_client(stream, clients, fnc, methods).await });
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum RpcApiRequest {    
    Identify {
        token: String,
        public_key: Vec<u8>,
    },
    Heartbeat {},
    GetId {},
    Message {
        id: String,
        method: String,
        data: Value, 
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HelloEvent {
    public_key: Vec<u8>,
    request_ids: Vec<String>,
}

async fn start_client(
    stream: Result<TcpStream, std::io::Error>,
    clients: Arc<DashMap<String, RpcClient>>,
    authenticate: AuthenticateFn,
    methods: Arc<DashMap<String, Box<dyn MethodFn>>>,
) {
    let connection = stream.unwrap();
    println!("Socket connected: {}", connection.peer_addr().unwrap());
    let ws_stream = accept_async(connection).await.expect("Failed to accept");
    let (mut write, mut read) = ws_stream.split();
    let (s, r) = unbounded::<Message>();
    spawn(async move {
        while let Ok(msg) = r.recv().await {
            write.send(msg).await.expect("Failed to send message");
        }
        write.close().await.expect("Failed to close");
    });
    let id = generate_id();
    let mut request_ids = Vec::new();
    for _ in 0..20 {
        request_ids.push(generate_id());
    }
    let secret = EphemeralSecret::random_from_rng(OsRng);
    let public_key = PublicKey::from(&secret);
    let val = HelloEvent {
        public_key: public_key.to_bytes().to_vec(),
        request_ids: request_ids.clone(),
    };
    s.send(Message::Binary(
        serialize(&val).expect("Failed to serialize"),
    ))
    .await
    .expect("Failed to send message");

    let (tx, rx) = unbounded::<()>();
    let clients_moved = clients.clone();
    let id_moved = id.clone();
    spawn(async move {
        while future::timeout(
            std::time::Duration::from_millis(*HEARTBEAT_TIMEOUT),
            rx.recv(),
        )
        .await
        .is_ok()
        {}
        if let Some((_, client)) = clients_moved.remove(&id_moved) {
            client.socket.close();
        }
    });
    let client = RpcClient {
        id: id.clone(),
        socket: Arc::new(s),
        user: None,
        request_ids,
        heartbeat_tx: Arc::new(tx),
    };
    clients.insert(id.clone(), client);
    while let Some(data) = read.next().await {
        match data.unwrap() {
            Message::Binary(bin) => {
                debug!("Received binary data");
                let response = handle_packet(bin, &clients, &id, authenticate.clone(), methods.clone()).await;
                let client = clients.get(&id.clone()).unwrap();
                client.send(serialize(&response).expect("Failed to serialize")).await;
            }
            Message::Close(_) => {
                debug!("Received close");
            }
            _ => {
                debug!("Received unknown message");
                if let Some((_, client)) = clients.remove(&id.clone()) {
                    client.socket.close();
                }
            }
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RpcApiResponse {
    id: Option<String>,
    response: Option<Value>,
}

#[derive(Clone, Debug, Serialize)]
pub struct IdentifyResponse {}

#[derive(Clone, Debug, Serialize)]
pub struct HeartbeatResponse {}

#[derive(Clone, Debug, Serialize)]
pub struct GetIdResponse {
    request_ids: Vec<String>,
}

impl Into<Value> for Error {
    fn into(self) -> Value {
        to_value(&self).unwrap()
    }
}

#[derive(Clone, Debug, Serialize)]
struct RpcApiError {
    error: Error,
}

impl Into<Value> for RpcApiError {
    fn into(self) -> Value {
        to_value(&self).unwrap()
    }
}

pub async fn handle_packet(
    bin: Vec<u8>,
    clients: &Arc<DashMap<String, RpcClient>>,
    user_id: &String,
    authenticate: AuthenticateFn,
    methods: Arc<DashMap<String, Box<dyn MethodFn>>>,
) -> RpcApiResponse {
    let result = deserialize::<RpcApiRequest>(bin.as_slice());
    if let Ok(r) = result {
        debug!("Received: {:?}", r);
        match r {
            RpcApiRequest::Identify { token, public_key: _ } => {
                authenticate(token.clone()).await.map(|user| {
                    let mut client = clients.get_mut(user_id).unwrap();
                    client.user = Some(Arc::new(user));
                    let response = IdentifyResponse {};
                    return RpcApiResponse {
                        id: None,
                        response: Some(to_value(response).unwrap()),
                    };
                }).unwrap_or_else(|e| RpcApiResponse {
                    id: None,
                    response: Some(RpcApiError { error: e.into() }.into()),
                })
            },
            RpcApiRequest::Heartbeat {  } => {
                let client = clients.get(user_id).unwrap();
                client.heartbeat_tx.send(()).await.unwrap();
                let response = HeartbeatResponse {};
                RpcApiResponse {  
                    response: Some(to_value(response).unwrap()),
                    id: None,
                }
            },
            RpcApiRequest::GetId {  } => {
                let mut client = clients.get_mut(user_id).unwrap();
                let mut new_request_ids = Vec::new();
                for _ in 0..20 {
                    let id = generate_id();
                    client.request_ids.push(id.clone());
                    new_request_ids.push(id);
                }
                let response = GetIdResponse {
                    request_ids: new_request_ids,
                };
                RpcApiResponse {
                    response: Some(to_value(response).unwrap()),
                    id: None,
                }
            },
            RpcApiRequest::Message { id, method, data } => {
                let method = methods.get(&method);
                let Some(method) = method else {
                    return RpcApiResponse {
                        id: Some(id),
                        response: Some(RpcApiError { error: Error::InvalidMethod }.into()),
                    };
                };
                let result = method(clients.clone(), id.clone(), data).await;
                RpcApiResponse {
                    id: Some(id),
                    response: Some(result),
                }
            },
        }
    } else {
        RpcApiResponse {
            id: None,
            response: Some(RpcApiError { error: Error::InvalidMethod }.into()),
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

pub fn emit_all<T:Serialize+Send+Clone + 'static>(clients: &DashMap<String, RpcClient>, data: T) {
    for client in clients.iter() {
        emit_one(client.value(), data.clone());
    }
}
pub fn emit_one<T:Serialize+Send+Clone + 'static>(client: &RpcClient, data: T) {
    let socket = client.socket.clone();
    let data = data.clone();
    spawn(async move {
        socket.send(Message::Binary(serialize(&data).expect("Failed to serialize"))).await
    });
}
