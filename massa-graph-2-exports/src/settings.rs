use massa_time::MassaTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GraphConfig {
    pub clock_compensation_millis: i64,
    pub thread_count: u8,
    pub genesis_timestamp: MassaTime,
    pub t0: MassaTime,
}
