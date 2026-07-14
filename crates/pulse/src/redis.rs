use std::sync::{LazyLock, OnceLock};

use redis::{Client, aio::MultiplexedConnection};
use ulid::Ulid;

use crate::environment::REDIS_URI;

static REDIS: OnceLock<Client> = OnceLock::new();
pub static INSTANCE_ID: LazyLock<String> = LazyLock::new(|| Ulid::new().to_string());

pub fn connect() {
    let client = Client::open(&**REDIS_URI).expect("Failed to connect");
    REDIS.set(client).expect("Failed to set client");
}

pub fn get_client() -> &'static Client {
    REDIS.get().expect("Failed to get client")
}

pub async fn get_connection() -> MultiplexedConnection {
    get_client()
        .get_multiplexed_async_connection()
        .await
        .expect("Failed to get connection")
}
