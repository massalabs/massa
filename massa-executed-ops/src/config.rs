//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::num::NonZeroU8;

#[derive(Debug, Clone)]
pub struct ExecutedOpsConfig {
    /// Number of threads
    pub thread_count: NonZeroU8,
    /// Maximum size of a bootstrap part
    pub bootstrap_part_size: u64,
}
