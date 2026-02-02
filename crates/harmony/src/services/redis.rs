use once_cell::sync::{Lazy, OnceCell};
use redis::{AsyncCommands, Client, aio::MultiplexedConnection};
use ulid::Ulid;

use super::environment::REDIS_URI;

static REDIS: OnceCell<Client> = OnceCell::new();
pub static INSTANCE_ID: Lazy<String> = Lazy::new(|| Ulid::new().to_string());

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

pub async fn get_pubsub() -> redis::aio::PubSub {
    get_client()
        .get_async_pubsub()
        .await
        .expect("Failed to get connection")
}

pub async fn create_streams() -> redis::RedisResult<()> {
    let mut conn = get_connection().await;
    let _ = conn
        .xgroup_create_mkstream::<&str, &str, &str, ()>(
            "voice:events:user-lifecycle",
            "harmony-servers",
            "0",
        )
        .await;

    Ok(())
}
