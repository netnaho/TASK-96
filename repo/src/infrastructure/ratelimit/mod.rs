/// In-memory rate limiters backed by `governor`.
///
/// ## Limits applied
///
/// | Limiter        | Key        | Quota            | Applied at             |
/// |----------------|------------|------------------|------------------------|
/// | `ip_limiter`   | client IP  | 300 req / minute | All routes (middleware) |
/// | `auth_limiter` | client IP  | 10 req / minute  | Login + captcha routes |
/// | `user_limiter` | user UUID  | 60 req / minute  | Authenticated routes   |
///
/// ## Thread safety
///
/// All limiters are `Arc`-wrapped `DashMap`-backed `DefaultKeyedRateLimiter`.
/// Safe to clone and share across actix worker threads.
///
/// ## Persistence
///
/// Rate limit counters are in-memory only. They reset on process restart.
/// For stateless horizontal scale, a Redis-backed limiter would be required —
/// that is outside the scope of this phase and documented in architecture.md.
use std::sync::Arc;

use governor::{
    clock::{Clock, DefaultClock},
    state::{keyed::DefaultKeyedStateStore, NotKeyed},
    Quota, RateLimiter,
};
use std::num::NonZeroU32;

/// Keyed rate limiter alias used for IP and user limits.
pub type KeyedLimiter = Arc<RateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>>;

/// Non-keyed limiter alias (not currently used globally, kept for future use).
pub type GlobalLimiter = Arc<RateLimiter<NotKeyed, governor::state::InMemoryState, DefaultClock>>;

/// All rate limiters used by the application.
#[derive(Clone)]
pub struct RateLimiters {
    /// Per-IP: 300 requests per minute across all routes.
    pub ip: KeyedLimiter,
    /// Per-IP: 10 requests per minute on login and captcha endpoints.
    pub auth_ip: KeyedLimiter,
    /// Per-user: 60 requests per minute on authenticated routes.
    pub user: KeyedLimiter,
}

impl RateLimiters {
    pub fn new() -> Self {
        Self {
            ip: Arc::new(RateLimiter::keyed(Quota::per_minute(
                NonZeroU32::new(300).unwrap(),
            ))),
            auth_ip: Arc::new(RateLimiter::keyed(Quota::per_minute(
                NonZeroU32::new(10).unwrap(),
            ))),
            user: Arc::new(RateLimiter::keyed(Quota::per_minute(
                NonZeroU32::new(60).unwrap(),
            ))),
        }
    }
}

impl Default for RateLimiters {
    fn default() -> Self {
        Self::new()
    }
}

/// Check the per-IP general limit.
/// Returns `Err(retry_after_secs)` if the IP is rate-limited.
pub fn check_ip(limiters: &RateLimiters, ip: &str) -> Result<(), u64> {
    match limiters.ip.check_key(&ip.to_string()) {
        Ok(_) => Ok(()),
        Err(not_until) => {
            let secs = not_until
                .wait_time_from(DefaultClock::default().now())
                .as_secs();
            Err(secs)
        }
    }
}

/// Check the per-IP auth limit (login/captcha endpoints).
pub fn check_auth_ip(limiters: &RateLimiters, ip: &str) -> Result<(), u64> {
    match limiters.auth_ip.check_key(&ip.to_string()) {
        Ok(_) => Ok(()),
        Err(not_until) => {
            let secs = not_until
                .wait_time_from(DefaultClock::default().now())
                .as_secs();
            Err(secs)
        }
    }
}

/// Check the per-user limit on authenticated routes.
pub fn check_user(limiters: &RateLimiters, user_id: &str) -> Result<(), u64> {
    match limiters.user.check_key(&user_id.to_string()) {
        Ok(_) => Ok(()),
        Err(not_until) => {
            let secs = not_until
                .wait_time_from(DefaultClock::default().now())
                .as_secs();
            Err(secs)
        }
    }
}
