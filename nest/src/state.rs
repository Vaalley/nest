//! Shared application state injected into every request handler.

use std::sync::Arc;

use sqlx::SqlitePool;

use crate::config::Config;
use crate::rate_limit::{RateLimiter, DEFAULT_MAX_REQUESTS, DEFAULT_WINDOW};
use crate::repository::{BirdRepository, ClutchRepository, EggRepository, FlockRepository};

/// Cloneable application state (cheap: everything behind `Arc`/pool handle).
#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

struct Inner {
    config: Config,
    pool: SqlitePool,
    rate_limiter: RateLimiter,
    flocks: FlockRepository,
    birds: BirdRepository,
    clutches: ClutchRepository,
    eggs: EggRepository,
}

impl AppState {
    pub fn new(config: Config, pool: SqlitePool) -> Self {
        let rate_limiter = RateLimiter::new(DEFAULT_WINDOW, DEFAULT_MAX_REQUESTS);
        let flocks = FlockRepository::new(pool.clone());
        let birds = BirdRepository::new(pool.clone());
        let clutches = ClutchRepository::new(pool.clone());
        let eggs = EggRepository::new(pool.clone());

        Self {
            inner: Arc::new(Inner {
                config,
                pool,
                rate_limiter,
                flocks,
                birds,
                clutches,
                eggs,
            }),
        }
    }

    pub fn config(&self) -> &Config {
        &self.inner.config
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.inner.pool
    }

    pub fn rate_limiter(&self) -> &RateLimiter {
        &self.inner.rate_limiter
    }

    pub fn flocks(&self) -> &FlockRepository {
        &self.inner.flocks
    }

    pub fn birds(&self) -> &BirdRepository {
        &self.inner.birds
    }

    pub fn clutches(&self) -> &ClutchRepository {
        &self.inner.clutches
    }

    pub fn eggs(&self) -> &EggRepository {
        &self.inner.eggs
    }
}
