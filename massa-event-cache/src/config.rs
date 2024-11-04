use std::path::PathBuf;

pub struct EventCacheConfig {
    /// Path to the hard drive cache storage
    pub event_cache_path: PathBuf,
    /// Maximum number of entries we want to keep in the event cache
    pub max_event_cache_length: usize,
    /// Amount of entries removed when `event_cache_size` is reached
    pub snip_amount: usize,
    /// Maximum length of an event data (aka event message)
    pub max_event_data_length: u64,
    /// Thread count
    pub thread_count: u8,
    /// Call stack max length
    pub max_recursive_call_depth: u16,
}
