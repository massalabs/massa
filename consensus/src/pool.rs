use std::collections::{BinaryHeap, HashMap, HashSet};

use crypto::{hash::Hash, signature::PublicKey, signature::SignatureEngine};
use models::{Operation, SerializationContext, Slot};

use crate::{ConsensusConfig, ConsensusError};

#[derive(Eq, PartialEq, Ord, PartialOrd, Copy, Clone, Hash, Debug)]
pub struct Adress(Hash); // Public key hash

impl Adress {
    fn from_public_key(key: PublicKey) -> Adress {
        todo!()
    }

    fn get_thread(&self, thread_count: u8) -> u8 {
        todo!()
    }
}

#[derive(Eq, PartialEq, Ord, PartialOrd, Copy, Clone, Hash, Debug)]
pub struct OperationId(Hash); // Signature hash

pub struct OperationPool {
    map: HashMap<OperationId, Operation>,
    /// one vec per thread
    vec: Vec<BinaryHeap<(u64, OperationId)>>, // fee operation hash
    current_slot: Slot,
    cfg: ConsensusConfig,
}

impl OperationPool {
    pub fn new(slot: Slot, cfg: ConsensusConfig) -> OperationPool {
        OperationPool {
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
        operation: Operation,
        context: &SerializationContext,
    ) -> Result<bool, ConsensusError> {
        let signature_engine = SignatureEngine::new();
        let hash = operation.content.compute_hash(context)?;
        // period validity check
        let start = operation.content.expiration_period - self.cfg.operation_validity_periods;
        let thread = get_thread(operation.content.creator_public_key);
        if self.current_slot < Slot::new(start, thread)
            || Slot::new(operation.content.expiration_period, thread) < self.current_slot
        {
            return Ok(false);
        }
        signature_engine.verify(
            &hash,
            &operation.signature,
            &operation.content.creator_public_key,
        )?;
        let id = OperationId(Hash::hash(&operation.signature.to_bytes()));
        if let None = self.map.insert(id, operation.clone()) {
            self.vec[thread as usize].push((operation.content.fee, id));
            Ok(true)
        } else {
            // already present
            Ok(false)
        }
        // todo!() // see #270
    }

    /// Update current_slot and discard invalid or integrated operation
    pub fn ack_final_block(&mut self, block_slot: Slot, thread_final_ops: HashSet<Hash>) {
        todo!() // see #270
    }

    /// Get max_count operation for thread block_slot.thread
    /// if vec is not full that means that there is no more interesting transactions left
    pub fn get_ops(
        &mut self,
        block_slot: Slot,
        exclude: HashSet<OperationId>,
        max_count: usize,
    ) -> Result<Vec<(OperationId, Operation)>, ConsensusError> {
        let mut res = self.vec[block_slot.thread as usize]
            .clone()
            .into_iter_sorted()
            .filter(|(_, hash)| !exclude.contains(hash))
            .map(|(_, hash)| {
                Ok((
                    hash,
                    self.map
                        .get(&hash)
                        .ok_or(ConsensusError::ContainerInconsistency(
                            "inconsistency between vec pool and map pool".into(),
                        ))?
                        .clone(),
                ))
            });
        if let Some(err) = res.find_map(|r: Result<(OperationId, Operation), ConsensusError>| {
            if r.is_err() {
                Some(r.unwrap_err())
            } else {
                None
            }
        }) {
            Err(err)
        } else {
            Ok(res.flatten().collect::<Vec<_>>()[0..max_count].into())
        }
    }
}

fn get_thread(key: PublicKey) -> u8 {
    todo!()
}
