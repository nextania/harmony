use std::time::Duration;

use async_trait::async_trait;
use rapid::rate_limit::RateLimiter;
use redis::AsyncCommands;

use super::redis::get_connection;

const WRITE_METHODS: &[&str] = &[
    "SEND_MESSAGE",
    "CREATE_CHANNEL",
    "CREATE_INVITE",
    "EDIT_MESSAGE",
    "EDIT_CHANNEL",
];

const GLOBAL_INTERVAL: Duration = Duration::from_secs(60);
const GLOBAL_MAX_REQUESTS: u64 = 120;

const WRITE_INTERVAL: Duration = Duration::from_secs(60);
const WRITE_MAX_REQUESTS: u64 = 30;

pub struct RedisRateLimiter;

#[async_trait]
impl RateLimiter for RedisRateLimiter {
    async fn check_rate_limit(&self, user_id: &str, method: &str) -> bool {
        let global_key = format!("rl:{}:global", user_id);
        if !check_and_record(&global_key, GLOBAL_INTERVAL, GLOBAL_MAX_REQUESTS).await {
            return false;
        }
        if WRITE_METHODS.contains(&method) {
            let method_key = format!("rl:{}:{}", user_id, method);
            if !check_and_record(&method_key, WRITE_INTERVAL, WRITE_MAX_REQUESTS).await {
                return false;
            }
        }

        true
    }
}

fn get_time_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

async fn check_and_record(key: &str, interval: Duration, max_requests: u64) -> bool {
    let mut conn = get_connection().await;
    let now = get_time_millis();
    let window_start = now - (interval.as_millis() as u64);
    let _: Result<(), _> = conn.zrembyscore(key, 0u64, window_start).await;
    let count: u64 = match conn.zcard(key).await {
        Ok(c) => c,
        Err(_) => return true, // fail open
    };
    if count >= max_requests {
        return false;
    }
    let _: Result<(), _> = conn.zadd(key, now, now).await;
    let ttl_secs = interval.as_secs() + 5;
    let _: Result<(), _> = conn.expire(key, ttl_secs as i64).await;

    true
}
