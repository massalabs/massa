use std::collections::{BinaryHeap, HashMap, HashSet};

use crypto::{hash::Hash, signature::PublicKey, signature::SignatureEngine};
use models::{Operation, SerializationContext, Slot};

use crate::{ConsensusConfig, ConsensusError};

pub struct Pool {
    map: HashMap<Hash, Operation>,
    /// one vec per thread
    vec: Vec<BinaryHeap<(u64, Hash)>>, // fee operation hash
    current_slot: Slot,
    cfg: ConsensusConfig,
}

impl Pool {
    pub fn new(slot: Slot, cfg: ConsensusConfig) -> Pool {
        Pool {
            map: HashMap::new(),
            vec: vec![BinaryHeap::new(); cfg.thread_count as usize],
            current_slot: slot,
            cfg,
        }
    }

    /// Incomming operations
    ///
    /// Promote operation if already present.
    /// Else check validity and then insert
    /// * current_slot in validity period
    /// * signature ok
    /// Ask new operation for propagation
    ///
    /// An error is returned when a critically wrrong operation was received
    pub fn new_operation(
        &mut self,
        new: Operation,
        context: &SerializationContext,
    ) -> Result<bool, ConsensusError> {
        let operation = new.clone();
        let signature_engine = SignatureEngine::new();
        match new {
            Operation::Transaction { content, signature } => {
                let hash = content.compute_hash(context)?;
                // period validity check
                let start = content.expire_period - self.cfg.validity_period;
                let thread = get_thread(content.sender_public_key);
                if self.current_slot < Slot::new(start, thread)
                    || Slot::new(content.expire_period, thread) < self.current_slot
                {
                    return Ok(false);
                }
                signature_engine.verify(&hash, &signature, &content.sender_public_key)?;
                if let None = self.map.insert(hash, operation) {
                    self.vec[thread as usize].push((content.fee, hash));
                    Ok(true)
                } else {
                    // already present
                    Ok(false)
                }
            }
        }
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

fn get_thread(key: PublicKey) -> u8 {
    todo!()
}
