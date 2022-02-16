use massa_time::MassaTime;
use std::path::PathBuf;

/// VM module configuration
#[derive(Debug, Clone)]
pub struct VMConfig {
    /// Initial SCE ledger file
    pub initial_sce_ledger_path: PathBuf,
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
