use crate::config::EventCacheConfig;
use crate::event_cache::EventCache;
use massa_models::execution::EventFilter;
use massa_models::output_event::SCOutputEvent;

/// Final event cache controller
pub struct EventCacheController {
    #[allow(dead_code)]
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
        let event_cache = EventCache::new(
            &cfg.event_cache_path,
            cfg.max_event_cache_length,
            cfg.snip_amount,
            cfg.thread_count,
            cfg.max_recursive_call_depth,
            cfg.max_event_data_length,
        );
        Self { cfg, event_cache }
    }

    pub fn save_events(&mut self, events: impl ExactSizeIterator<Item = SCOutputEvent> + Clone) {
        self.event_cache.insert_multi_it(events);
    }

    pub fn get_filtered_sc_output_events<'b, 'a: 'b>(
        &'a self,
        filter: &'b EventFilter,
    ) -> impl Iterator<Item = SCOutputEvent> + 'b {
        self.event_cache.get_filtered_sc_output_events(filter)
    }
}
