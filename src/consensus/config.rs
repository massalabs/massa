use crate::crypto::signature::{PrivateKey, PublicKey};
use crate::structures::block::Block;
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct ConsensusConfig {
    pub genesis_timestamp_millis: u64,
    pub thread_count: u8,
    pub t0_millis: u64,
    pub selection_rng_seed: u64,
    pub genesis_key: PrivateKey,
    pub nodes: Vec<(PublicKey, PrivateKey)>,
    pub current_node_index: u32,
}
