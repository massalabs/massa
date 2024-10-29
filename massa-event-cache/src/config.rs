use std::path::PathBuf;

pub struct EventCacheConfig {
    /// Path to the hard drive cache storage
    pub event_cache_path: PathBuf,
    /// Maximum number of entries we want to keep in the event cache
    pub event_cache_size: usize,
    /// Amount of entries removed when `event_cache_size` is reached
    pub snip_amount: usize,
    /// Maximum length of an event
    pub max_event_length: u64,
    /// Thread count
    pub thread_count: u8,
}
