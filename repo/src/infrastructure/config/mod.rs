use std::env;

/// Application configuration, fully driven by environment variables.
/// All fields have sensible defaults for local/compose development.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub host: String,
    pub port: u16,
    pub storage_path: String,
    pub encryption_key: String,
    pub session: SessionConfig,
    pub rate_limit: RateLimitConfig,
    pub lockout: LockoutConfig,
    pub scheduler: SchedulerConfig,
    pub reporting_delivery: ReportingDeliveryConfig,
    pub run_migrations: bool,
    pub run_seed: bool,
}

/// Session lifetime settings.
///
/// `ttl_seconds` is the **hard** expiry: the session is unconditionally invalid
/// after this many seconds regardless of activity.  It should be >= the idle
/// timeout (`SESSION_IDLE_TIMEOUT_SECS`, 8 h) so that active users are not
/// logged out unexpectedly.  Default: 28800 (8 hours).
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub ttl_seconds: u64,
    pub max_per_user: u32,
}

#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub requests_per_second: u32,
    pub burst_size: u32,
}

#[derive(Debug, Clone)]
pub struct LockoutConfig {
    pub threshold: u32,
    pub duration_seconds: u64,
    /// After this many consecutive failed login attempts on an account, CAPTCHA
    /// is mandatory on the next login attempt.  Default: `threshold - 2` (so
    /// with the default threshold of 5, CAPTCHA is required after 3 failures).
    /// Set to 0 to always require CAPTCHA, or to a value >= `threshold` to
    /// effectively disable mandatory CAPTCHA.
    pub captcha_required_after_failures: u32,
}

#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    pub enabled: bool,
    /// Local time of day for the daily reporting snapshot, e.g. `"06:00"`.
    /// Parsed as `HH:MM` (24-hour). Default: `"06:00"`.
    pub snapshot_time_local: String,
    /// IANA timezone name used to interpret `snapshot_time_local`.
    /// Examples: `"UTC"`, `"Africa/Addis_Ababa"`, `"America/New_York"`.
    /// Default: `"UTC"`.
    pub snapshot_timezone: String,
}

/// Local delivery gateway configuration.
///
/// Delivery is always local-network-only and best-effort — no third-party
/// services are required.  When `enabled = false` (the default) all delivery
/// attempts are skipped and the outcome is recorded as `"skipped"`.
///
/// | Variable                      | Default | Description                          |
/// |-------------------------------|---------|--------------------------------------|
/// | `REPORTING_DELIVERY_ENABLED`  | `false` | Master toggle for local delivery     |
/// | `REPORTING_EMAIL_GATEWAY_URL` | —       | `http://host:port/path` of local MTA |
/// | `REPORTING_IM_GATEWAY_URL`    | —       | `http://host:port/path` of local IM  |
#[derive(Debug, Clone)]
pub struct ReportingDeliveryConfig {
    pub enabled: bool,
    pub email_gateway_url: Option<String>,
    pub im_gateway_url: Option<String>,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            database_url: required_env("DATABASE_URL"),
            host: env_or("APP_HOST", "127.0.0.1"),
            port: env_or("APP_PORT", "8080")
                .parse()
                .expect("APP_PORT must be a number"),
            storage_path: env_or("STORAGE_PATH", "./storage"),
            encryption_key: required_env("ENCRYPTION_KEY"),
            session: SessionConfig {
                // Default matches SESSION_IDLE_TIMEOUT_SECS (8 h) so the hard
                // expiry never kills an active session before the idle timeout.
                ttl_seconds: env_or("SESSION_TTL_SECONDS", "28800")
                    .parse()
                    .expect("SESSION_TTL_SECONDS must be a number"),
                max_per_user: env_or("SESSION_MAX_PER_USER", "5")
                    .parse()
                    .expect("SESSION_MAX_PER_USER must be a number"),
            },
            rate_limit: RateLimitConfig {
                requests_per_second: env_or("RATE_LIMIT_RPS", "30")
                    .parse()
                    .expect("RATE_LIMIT_RPS must be a number"),
                burst_size: env_or("RATE_LIMIT_BURST", "60")
                    .parse()
                    .expect("RATE_LIMIT_BURST must be a number"),
            },
            lockout: {
                let threshold: u32 = env_or("LOCKOUT_THRESHOLD", "5")
                    .parse()
                    .expect("LOCKOUT_THRESHOLD must be a number");
                let captcha_default = threshold.saturating_sub(2).to_string();
                LockoutConfig {
                    threshold,
                    duration_seconds: env_or("LOCKOUT_DURATION_SECONDS", "900")
                        .parse()
                        .expect("LOCKOUT_DURATION_SECONDS must be a number"),
                    captcha_required_after_failures: env_or(
                        "CAPTCHA_REQUIRED_AFTER_FAILURES",
                        &captcha_default,
                    )
                    .parse()
                    .expect("CAPTCHA_REQUIRED_AFTER_FAILURES must be a number"),
                }
            },
            scheduler: SchedulerConfig {
                enabled: env_or("SCHEDULER_ENABLED", "false")
                    .parse()
                    .expect("SCHEDULER_ENABLED must be true/false"),
                snapshot_time_local: env_or("SNAPSHOT_TIME_LOCAL", "06:00"),
                snapshot_timezone: env_or("SNAPSHOT_TIMEZONE", "UTC"),
            },
            reporting_delivery: ReportingDeliveryConfig {
                enabled: env_or("REPORTING_DELIVERY_ENABLED", "false")
                    .parse()
                    .expect("REPORTING_DELIVERY_ENABLED must be true/false"),
                email_gateway_url: env::var("REPORTING_EMAIL_GATEWAY_URL").ok(),
                im_gateway_url: env::var("REPORTING_IM_GATEWAY_URL").ok(),
            },
            run_migrations: env_or("RUN_MIGRATIONS", "false")
                .parse()
                .expect("RUN_MIGRATIONS must be true/false"),
            run_seed: env_or("RUN_SEED", "false")
                .parse()
                .expect("RUN_SEED must be true/false"),
        }
    }
}

fn required_env(key: &str) -> String {
    env::var(key).unwrap_or_else(|_| panic!("required environment variable {key} is not set"))
}

fn env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

// ============================================================
// Seed configuration helpers
// ============================================================

/// A resolved seed user password and whether it was auto-generated.
pub struct ResolvedPassword {
    pub value: String,
    pub was_generated: bool,
}

/// Resolve a seed password from a given value (typically from an env var).
/// If the value is `None` or empty, generates a random password.
pub fn resolve_seed_password(env_value: Option<&str>) -> ResolvedPassword {
    match env_value {
        Some(v) if !v.is_empty() => ResolvedPassword {
            value: v.to_string(),
            was_generated: false,
        },
        _ => ResolvedPassword {
            value: generate_seed_password(),
            was_generated: true,
        },
    }
}

/// Generate a random password suitable for seed users.
/// Format: `Seed!` prefix + 24 hex chars (meets complexity requirements).
fn generate_seed_password() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut buf);
    format!("Seed!{}", hex::encode(buf))
}
