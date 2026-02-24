use std::time::Duration;

use once_cell::sync::{Lazy, OnceCell};
use redis::{AsyncCommands, AsyncConnectionConfig, Client, aio::MultiplexedConnection};
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
        .get_multiplexed_async_connection_with_config(
            &AsyncConnectionConfig::default().set_response_timeout(Some(Duration::from_secs(10))),
        )
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
    let e = conn
        .xgroup_create_mkstream::<&str, &str, &str, ()>(
            "voice:events:user-lifecycle",
            "harmony-servers",
            "0",
        )
        .await;
    if let Err(e) = e
        && !e.to_string().contains("BUSYGROUP")
    {
        return Err(e);
    }

    Ok(())
}

pub async fn set_user_online(user_id: &str) -> redis::RedisResult<()> {
    let mut conn = get_connection().await;
    conn.set_ex(format!("user:{}:online", user_id), true, 60)
        .await
}

pub async fn set_user_offline(user_id: &str) -> redis::RedisResult<()> {
    let mut conn = get_connection().await;
    conn.del(format!("user:{}:online", user_id)).await
}

pub async fn is_user_online(user_id: &str) -> redis::RedisResult<bool> {
    let mut conn = get_connection().await;
    conn.exists(format!("user:{}:online", user_id)).await
}
