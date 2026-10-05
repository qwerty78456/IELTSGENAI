//! Per-minute request caps so a public URL cannot drain the API key.
//!
//! In-memory and per process: good enough for one VPS. Replace with a
//! per-user limiter once there is authentication.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    Topic,
    Passage,
    Task,
    Audio,
    /// A Gemini API key typed into the browser.
    KeyEntry,
    /// A paid voice sample ("Preview"); stored samples are free and not counted.
    VoiceSample,
    /// Creating or deleting a designed voice (Voice Design).
    VoiceDesign,
}

impl Bucket {
    /// Every bucket, in declaration order: `LIMITERS` is built from it and
    /// indexed by `bucket as usize`.
    pub const ALL: [Bucket; 7] = [
        Bucket::Topic,
        Bucket::Passage,
        Bucket::Task,
        Bucket::Audio,
        Bucket::KeyEntry,
        Bucket::VoiceSample,
        Bucket::VoiceDesign,
    ];

    fn per_minute(self) -> u32 {
        match self {
            Bucket::Topic => 20,
            Bucket::Passage => 15,
            Bucket::Task => 30,
            Bucket::Audio => 5,
            Bucket::KeyEntry => 5,
            Bucket::VoiceSample => 30,
            Bucket::VoiceDesign => 5,
        }
    }
}

struct RateLimiter {
    in_flight: Arc<AtomicU32>,
    max_per_minute: u32,
}

impl RateLimiter {
    fn new(max_per_minute: u32) -> Self {
        Self {
            in_flight: Arc::new(AtomicU32::new(0)),
            max_per_minute,
        }
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

static LIMITERS: OnceLock<[RateLimiter; Bucket::ALL.len()]> = OnceLock::new();

pub fn check(bucket: Bucket) -> Result<(), String> {
    let limiters =
        LIMITERS.get_or_init(|| Bucket::ALL.map(|bucket| RateLimiter::new(bucket.per_minute())));
    limiters[bucket as usize].check()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bucket_has_its_own_limiter() {
        for (index, bucket) in Bucket::ALL.into_iter().enumerate() {
            assert_eq!(bucket as usize, index, "{bucket:?}");
        }
        // The last variant closes the list: a variant added after it must
        // join ALL (and move this line) or indexing would go past the end.
        assert_eq!(Bucket::VoiceDesign as usize + 1, Bucket::ALL.len());
        assert_eq!(Bucket::VoiceSample.per_minute(), 30);
        assert_eq!(Bucket::VoiceDesign.per_minute(), 5);
    }
}
