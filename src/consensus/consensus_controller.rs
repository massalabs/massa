use super::config::ConsensusConfig;
use crate::crypto::hash::Hash;
use crate::structures::block::Block;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;

use tokio::sync::mpsc::{Receiver, Sender};

type BoxResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

struct ConsensusController {
    blocks: HashMap<Hash, Block>,
}

impl ConsensusController {
    fn new(cfg: ConsensusConfig) -> BoxResult<Self> {
        unimplemented!();
    }

    // TODO:
    // - Check if block is new
    // - Check if block is valid
    // if yes insert block in blocks and returns true
    fn new_block(&self, block: &Block) -> bool {
        true
    }

    fn create_block(&self) -> Block {
        unimplemented!();
    }
}
/// Communication with protocol_controller_fn
/// one incoming channel for new blocks
/// one incomming channel for block ids (protocol asking for a block)
/// one outgoing channel for Option(block) for the protocol to propagate
async fn consensus_controller_fn() {
    unimplemented!();
}
