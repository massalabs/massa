use std::collections::{HashMap, HashSet};

use crypto::{hash::Hash, signature::PublicKey, signature::SignatureEngine};
use models::{Operation, Slot};

use crate::ConsensusConfig;

pub struct Pool {
    map: HashMap<Hash, Operation>,
    /// one vec per thread
    vec: Vec<Vec<Hash>>,
    current_slot: Slot,
}

impl Pool {
    pub fn new(slot: Slot, cfg: ConsensusConfig) -> Pool {
        Pool {
            map: HashMap::new(),
            vec: vec![Vec::new(); cfg.thread_count as usize],
            current_slot: slot,
        }
    }

    /// Incomming operations
    ///
    /// Promote operation if already present.
    /// Else check validity and then insert
    /// * current_slot in validity period
    /// * signature ok
    /// Ask new operation for propagation
    pub fn new_operation(&mut self, new: Operation) -> bool {
        todo!()
    }

    /// Update current_slot and discard invalid or integrated operation
    pub fn ack_final_block(&mut self, block_slot: Slot, thread_final_ops: HashSet<Hash>) {
        todo!()
    }

    /// Get max_count operation for thread block_slot.thread
    /// if vec is not full that means that there is no more interesting transactions left
    pub fn get_ops(
        &mut self,
        block_slot: Slot,
        exclude: HashSet<Hash>,
        max_count: usize,
    ) -> Vec<(Hash, Operation)> {
        todo!()
    }
}
