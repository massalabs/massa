use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct ApiConfig {
    pub max_return_invalid_blocks: usize,
    pub selection_return_periods: u64,
}
