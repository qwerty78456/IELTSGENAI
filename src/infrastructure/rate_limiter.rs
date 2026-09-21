//! Per-minute request caps so a public URL cannot drain the API key.
//!
//! In-memory and per process: good enough for one VPS. Replace with a
//! per-user limiter once there is authentication.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

#[derive(Debug, Clone, Copy)]
pub enum Bucket {
    Topic,
    Passage,
    Task,
    Audio,
}

impl Bucket {
    fn per_minute(self) -> u32 {
        match self {
            Bucket::Topic => 20,
            Bucket::Passage => 15,
            Bucket::Task => 30,
            Bucket::Audio => 5,
        }
    }
}

struct RateLimiter {
    in_flight: Arc<AtomicU32>,
    max_per_minute: u32,
}

impl RateLimiter {
    fn new(max_per_minute: u32) -> Self {
        Self { in_flight: Arc::new(AtomicU32::new(0)), max_per_minute }
    }

    fn check(&self) -> Result<(), String> {
        let previous = self.in_flight.fetch_add(1, Ordering::SeqCst);
        if previous >= self.max_per_minute {
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            return Err(format!(
                "Rate limit exceeded: at most {} requests per minute. Please try again shortly.",
                self.max_per_minute
            ));
        }
        let counter = self.in_flight.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            counter.fetch_sub(1, Ordering::SeqCst);
        });
        Ok(())
    }
}

static LIMITERS: OnceLock<[RateLimiter; 4]> = OnceLock::new();

pub fn check(bucket: Bucket) -> Result<(), String> {
    let limiters = LIMITERS.get_or_init(|| {
        [
            RateLimiter::new(Bucket::Topic.per_minute()),
            RateLimiter::new(Bucket::Passage.per_minute()),
            RateLimiter::new(Bucket::Task.per_minute()),
            RateLimiter::new(Bucket::Audio.per_minute()),
        ]
    });
    limiters[bucket as usize].check()
}
