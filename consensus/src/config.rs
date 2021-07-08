use crypto::signature::{PrivateKey, PublicKey};
use serde::Deserialize;
use std::default::Default;
use time::UTime;

#[derive(Debug, Deserialize, Clone)]
pub struct ConsensusConfig {
    pub genesis_timestamp: UTime,
    pub thread_count: u8,
    pub t0: UTime,
    pub selection_rng_seed: u64,
    pub genesis_key: PrivateKey,
    pub nodes: Vec<(PublicKey, PrivateKey)>,
    pub current_node_index: u32,
    pub max_discarded_blocks: usize,
    pub future_block_processing_max_periods: u64,
    pub max_future_processing_blocks: usize,
    pub max_dependency_blocks: usize,
    pub delta_f0: u64,

    //parameter that shouldn't be defined in prod.
    #[serde(skip, default = "Default::default")]
    pub disable_block_creation: bool,
}
