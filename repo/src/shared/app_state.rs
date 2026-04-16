use crate::infrastructure::{captcha, config::AppConfig, db::DbPool, ratelimit::RateLimiters};

/// Shared application state injected into every actix-web handler via `web::Data<AppState>`.
///
/// Cloning is cheap — `DbPool` and `RateLimiters` are already `Arc`-wrapped internally.
#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub db_pool: DbPool,
    pub rate_limiters: RateLimiters,
    /// 32-byte key derived from `ENCRYPTION_KEY`, used only for CAPTCHA HMAC.
    pub captcha_key: [u8; 32],
}

impl AppState {
    pub fn new(config: AppConfig, db_pool: DbPool) -> Result<Self, String> {
        let captcha_key = captcha::derive_captcha_key(&config.encryption_key)?;
        Ok(Self {
            rate_limiters: RateLimiters::new(),
            captcha_key,
            config,
            db_pool,
        })
    }
}
