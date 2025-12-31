//! Simple rate limiter to prevent API abuse

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

#[cfg(target_arch = "wasm32")]
use gloo_timers::callback::Timeout;

/// Simple in-memory rate limiter
/// Limits requests per minute to prevent API abuse when exposed publicly
pub struct RateLimiter {
    #[cfg(not(target_arch = "wasm32"))]
    requests_this_minute: Arc<AtomicU32>,
    #[cfg(target_arch = "wasm32")]
    requests_this_minute: Arc<AtomicU32>,
    max_requests_per_minute: u32,
}

impl RateLimiter {
    pub fn new(max_requests_per_minute: u32) -> Self {
        Self {
            requests_this_minute: Arc::new(AtomicU32::new(0)),
            max_requests_per_minute,
        }
    }

    /// Check if a request is allowed
    /// Returns Ok(()) if allowed, Err with retry message if rate limited
    pub fn check_rate_limit(&self) -> Result<(), String> {
        let current = self.requests_this_minute.load(Ordering::Relaxed);

        if current >= self.max_requests_per_minute {
            return Err(format!(
                "Rate limit exceeded. Maximum {} requests per minute. Please try again later.",
                self.max_requests_per_minute
            ));
        }

        self.requests_this_minute.fetch_add(1, Ordering::Relaxed);

        // Reset counter after 60 seconds
        #[cfg(target_arch = "wasm32")]
        {
            let counter = self.requests_this_minute.clone();
            Timeout::new(60_000, move || {
                counter.fetch_sub(1, Ordering::Relaxed);
            })
            .forget();
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let counter = self.requests_this_minute.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                counter.fetch_sub(1, Ordering::Relaxed);
            });
        }

        Ok(())
    }
}

use std::sync::OnceLock;

// Global rate limiters for each service (thread-safe initialization)
static TOPIC_LIMITER: OnceLock<RateLimiter> = OnceLock::new();
static SCRIPT_LIMITER: OnceLock<RateLimiter> = OnceLock::new();
static AUDIO_LIMITER: OnceLock<RateLimiter> = OnceLock::new();

/// Initialize rate limiters (call once at startup)
pub fn init_rate_limiters() {
    // Conservative limits to prevent abuse
    // Topic generation: 20/min (cheap, fast)
    let _ = TOPIC_LIMITER.set(RateLimiter::new(20));
    // Script generation: 15/min (moderate cost)
    let _ = SCRIPT_LIMITER.set(RateLimiter::new(15));
    // Audio generation: 5/min (expensive, slow)
    let _ = AUDIO_LIMITER.set(RateLimiter::new(5));
}

pub fn check_topic_rate_limit() -> Result<(), String> {
    TOPIC_LIMITER
        .get_or_init(|| RateLimiter::new(20))
        .check_rate_limit()
}

pub fn check_script_rate_limit() -> Result<(), String> {
    SCRIPT_LIMITER
        .get_or_init(|| RateLimiter::new(15))
        .check_rate_limit()
}

pub fn check_audio_rate_limit() -> Result<(), String> {
    AUDIO_LIMITER
        .get_or_init(|| RateLimiter::new(5))
        .check_rate_limit()
}
