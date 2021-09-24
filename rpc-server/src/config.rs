use serde::Deserialize;
#[derive(Debug, Deserialize, Clone)]
pub struct APIConfig {
    pub draw_lookahead_period_count: u64,
}
