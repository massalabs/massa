use serde::Deserialize;
use time::UTime;

pub const CHANNEL_SIZE: usize = 16;

#[derive(Clone, Debug, Deserialize)]
pub struct HangMonitorConfig {
    pub monitor_interval: UTime,
    pub hang_timeout: UTime,
    pub monitor: bool,
}
