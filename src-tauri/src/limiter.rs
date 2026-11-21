//! Token-bucket speed limiter for direct downloads. The rate can change at any time.

use parking_lot::Mutex;
use std::time::{Duration, Instant};

pub struct Limiter {
    inner: Mutex<Bucket>,
}

struct Bucket {
    /// Bytes per second, 0 = unlimited.
    rate: u64,
    tokens: f64,
    last: Instant,
}

impl Default for Limiter {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Limiter {
    pub fn new(rate: u64) -> Self {
        Self { inner: Mutex::new(Bucket { rate, tokens: 0.0, last: Instant::now() }) }
    }

    pub fn set_rate(&self, rate: u64) {
        let mut b = self.inner.lock();
        if b.rate != rate {
            b.rate = rate;
            b.tokens = b.tokens.min(rate as f64 / 4.0);
            b.last = Instant::now();
        }
    }

    /// How long to wait before `n` bytes may be used (taking them now).
    fn reserve(&self, n: u64) -> Option<Duration> {
        let mut b = self.inner.lock();
        if b.rate == 0 {
            return None;
        }
        let now = Instant::now();
        let elapsed = now.duration_since(b.last).as_secs_f64();
        b.last = now;
        // Allow at most a quarter second of burst.
        b.tokens = (b.tokens + elapsed * b.rate as f64).min(b.rate as f64 / 4.0);
        b.tokens -= n as f64;
        if b.tokens >= 0.0 {
            None
        } else {
            Some(Duration::from_secs_f64((-b.tokens) / b.rate as f64))
        }
    }

    pub async fn acquire(&self, n: u64) {
        if let Some(wait) = self.reserve(n) {
            tokio::time::sleep(wait.min(Duration::from_secs(5))).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn limits_rate() {
        let l = Limiter::new(100_000);
        let start = Instant::now();
        for _ in 0..30 {
            l.acquire(10_000).await;
        }
        // 300 KB at 100 KB/s ≈ 3 s, minus the initial burst allowance.
        let t = start.elapsed().as_secs_f64();
        assert!(t > 2.4 && t < 3.6, "took {t}");
        l.set_rate(0);
        let s2 = Instant::now();
        l.acquire(10_000_000).await;
        assert!(s2.elapsed() < Duration::from_millis(50));
    }
}
