//! In-memory per-IP rate limiter for authentication endpoints.
//!
//! This is intentionally basic: it tracks the number of requests from each IP
//! address inside a sliding window. It does not persist state, so a process
//! restart clears counters. It is sufficient for the MVP and keeps the Nest
//! dependency-free of a distributed rate-limiting backend.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::error::{AppError, AppResult};

/// Default sliding window for authentication endpoints.
pub const DEFAULT_WINDOW: Duration = Duration::from_secs(60);
/// Default maximum attempts per IP inside the window.
pub const DEFAULT_MAX_REQUESTS: usize = 5;

#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Inner>,
}

struct Inner {
    window: Duration,
    max_requests: usize,
    requests: Mutex<HashMap<IpAddr, Vec<Instant>>>,
}

impl RateLimiter {
    pub fn new(window: Duration, max_requests: usize) -> Self {
        Self {
            inner: Arc::new(Inner {
                window,
                max_requests,
                requests: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Record a request for `ip` and return an error if the rate limit is exceeded.
    pub fn check(&self, ip: IpAddr) -> AppResult<()> {
        let mut requests = self
            .inner
            .requests
            .lock()
            .map_err(|_| AppError::Internal("rate limiter poisoned".to_string()))?;

        let now = Instant::now();
        let entries = requests.entry(ip).or_default();
        entries.retain(|t| now.duration_since(*t) <= self.inner.window);

        if entries.len() >= self.inner.max_requests {
            return Err(AppError::RateLimited);
        }

        entries.push(now);
        Ok(())
    }
}
