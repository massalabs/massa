//! Copyright (c) 2023 MASSA LABS <info@massa.net>

#[derive(Debug, Clone)]
pub struct ExecutedOpsConfig {
    /// Number of threads
    pub thread_count: u8,
    /// Maximum size of a bootstrap part
    pub bootstrap_part_size: u64,
}

#[derive(Debug, Clone)]
pub struct ProcessedDenunciationsConfig {
    // /// Number of threads
    // pub thread_count: u8,
    // /// Maximum size of a bootstrap part
    // pub bootstrap_part_size: u64,
    pub denunciation_expire_periods: u64,
}
