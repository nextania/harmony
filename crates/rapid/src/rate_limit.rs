use async_trait::async_trait;

/// A trait for rate limiting RPC requests.
///
/// Implementations are responsible for checking whether a request from a given
/// user to a given method should be allowed, and recording the request if so.
/// `rapid` does not depend on any specific storage backend — the implementor
/// decides how state is persisted (e.g. Redis, in-memory, etc.).
#[async_trait]
pub trait RateLimiter: Send + Sync {
    /// Returns `true` if the request is allowed (and records it),
    /// or `false` if the caller has exceeded the rate limit.
    async fn check_rate_limit(&self, user_id: &str, method: &str) -> bool;
}
