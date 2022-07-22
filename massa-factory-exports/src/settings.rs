use massa_models::constants::THREAD_COUNT;
use serde::{Deserialize, Serialize};

/// Configuration of factory worker
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FactoryConfig {
    /// Number of running threads
    pub thread_count: u8,
    // todo: add configs, genesis timestamps, t0...
}

impl Default for FactoryConfig {
    fn default() -> Self {
        #[cfg(not(feature = "sandbox"))]
        let thread_count = THREAD_COUNT;
        #[cfg(feature = "sandbox")]
        let thread_count = *THREAD_COUNT;

        Self { thread_count }
    }
}
