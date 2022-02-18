use massa_time::MassaTime;

/// VM module configuration
#[derive(Debug, Clone)]
pub struct ExecutionConfig {
    /// read-only execution request queue length
    pub readonly_queue_length: usize,
    /// maximum number of SC output events kept in cache
    pub max_final_events: usize,
    /// number of threads
    pub thread_count: u8,
    /// extra lag to add on the cursor to improve performance
    pub cursor_delay: MassaTime,
    /// time compensation in milliseconds
    pub clock_compensation: i64,
    /// genesis timestamp
    pub genesis_timestamp: MassaTime,
    /// period duration
    pub t0: MassaTime,
}

