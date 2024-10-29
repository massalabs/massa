use crate::{
    config::EventCacheConfig,
};
use crate::event_cache::EventCache;

/// Final event cache controller
pub struct EventCacheController {
    /// Cache config.
    /// See `EventCacheConfig` documentation for more information.
    cfg: EventCacheConfig,
    /// Event stored cache.
    /// See the `EventCache` documentation for more information.
    event_cache: EventCache,
}

impl EventCacheController {
    /// Creates a new `EventCacheController`
    pub fn new(cfg: EventCacheConfig) -> Self {
        let event_cache= EventCache::new(
            cfg.event_cache_path.clone(),
            cfg.event_cache_size,
            cfg.snip_amount,
            cfg.thread_count,
        );
        Self {
            cfg,
            event_cache
        }
    }
}
