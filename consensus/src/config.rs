use crypto::signature::{PrivateKey, PublicKey};
use serde::Deserialize;
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
}
