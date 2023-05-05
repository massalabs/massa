//! Copyright (c) 2022 MASSA LABS <info@massa.net>

#[derive(Debug, Clone)]
pub struct ExecutedOpsConfig {
    /// Number of threads
    pub thread_count: u8,
    /// Maximum size of a bootstrap part
    pub bootstrap_part_size: u64,
}

#[derive(Debug, Clone)]
pub struct ExecutedDenunciationsConfig {
    /// Period delta for denunciation to expire
    pub denunciation_expire_periods: u64,
    /// Maximum size of a bootstrap part
    pub bootstrap_part_size: u64,
}
