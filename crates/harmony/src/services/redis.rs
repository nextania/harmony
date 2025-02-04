use once_cell::sync::OnceCell;
use redis::{aio::MultiplexedConnection, Client};

use super::environment::REDIS_URI;

static REDIS: OnceCell<Client> = OnceCell::new();

pub async fn connect() {
    let client = Client::open(&**REDIS_URI).expect("Failed to connect");
    REDIS.set(client).expect("Failed to set client");
}

pub fn get_client() -> &'static Client {
    REDIS.get().expect("Failed to get client")
}

pub async fn get_connection() -> MultiplexedConnection {
    get_client()
        .get_multiplexed_async_std_connection()
        .await
        .expect("Failed to get connection")
}

pub async fn get_pubsub() -> redis::aio::PubSub {
    get_client()
        .get_async_pubsub()
        .await
        .expect("Failed to get connection")
}
