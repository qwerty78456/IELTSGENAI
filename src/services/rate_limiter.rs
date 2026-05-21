//! Simple rate limiter to prevent API abuse
//! 
//! This module is now server-only.

#![cfg(feature = "server")]

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;

/// Simple in-memory rate limiter
/// Limits requests per minute to prevent API abuse when exposed publicly
pub struct RateLimiter {
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
        let prev = self.requests_this_minute.fetch_add(1, Ordering::SeqCst);
        if prev >= self.max_requests_per_minute {
            // Rollback — we exceeded the limit
            self.requests_this_minute.fetch_sub(1, Ordering::SeqCst);
            return Err(format!(
                "Rate limit exceeded. Maximum {} requests per minute. Please try again later.",
                self.max_requests_per_minute
            ));
        }

        let counter = self.requests_this_minute.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
            counter.fetch_sub(1, Ordering::SeqCst);
        });

        Ok(())
    }
}

// Global rate limiters for each service (thread-safe initialization)
static TOPIC_LIMITER: OnceLock<RateLimiter> = OnceLock::new();
static SCRIPT_LIMITER: OnceLock<RateLimiter> = OnceLock::new();
static AUDIO_LIMITER: OnceLock<RateLimiter> = OnceLock::new();

/// Initialize rate limiters (call once at startup)
pub fn init_rate_limiters() {
    // Rate limiters are lazily initialized via get_or_init() in each check function.
    // This function exists for forward-compatibility if eager init is needed later.
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
