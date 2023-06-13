//! Copyright (c) 2022 MASSA LABS <info@massa.net>

#[derive(Debug, Clone)]
pub struct ExecutedOpsConfig {
    /// Number of threads
    pub thread_count: u8,
}

#[derive(Debug, Clone)]
pub struct ExecutedDenunciationsConfig {
    /// Period delta for denunciation to expire
    pub denunciation_expire_periods: u64,
    /// Number of threads
    pub thread_count: u8,
    /// Number of endorsements
    pub endorsement_count: u32,
}
