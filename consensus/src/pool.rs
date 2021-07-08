use std::collections::{BTreeSet, BinaryHeap, HashMap, HashSet};

use crypto::{hash::Hash, signature::PublicKey, signature::SignatureEngine};
use models::{Operation, SerializationContext, SerializeCompact, Slot};
use num::rational::Ratio;

use crate::{ConsensusConfig, ConsensusError};

#[derive(Eq, PartialEq, Ord, PartialOrd, Copy, Clone, Hash, Debug)]
pub struct Address(Hash); // Public key hash

impl Address {
    fn new(key: PublicKey) -> Address {
        todo!()
    }

    fn get_thread(&self, thread_count: u8) -> u8 {
        todo!()
    }
}

struct WrappedOperation {
    op: Operation,
    byte_count: u64,
    thread: u8,
}

impl WrappedOperation {
    fn new(
        op: Operation,
        thread_count: u8,
        context: &SerializationContext,
    ) -> Result<Self, ConsensusError> {
        Ok(WrappedOperation {
            byte_count: op.to_bytes_compact(&context)?.len() as u64,
            thread: Address::new(op.content.creator_public_key).get_thread(thread_count),
            op,
        })
    }

    fn get_fee_density(&self) -> Ratio<u64> {
        Ratio::new(self.op.content.fee, self.byte_count)
    }

    fn is_valid_at_period(&self, period: u64, operation_validity_periods: u64) -> bool {
        let start = self.op.content.expiration_period - operation_validity_periods;
        Slot::new(period, self.thread) >= Slot::new(start, self.thread)
            && Slot::new(self.op.content.expiration_period, self.thread)
                >= Slot::new(period, self.thread)
    }
}

#[derive(Eq, PartialEq, Ord, PartialOrd, Copy, Clone, Hash, Debug)]
pub struct OperationId(Hash); // Signature hash

pub struct OperationPool {
    ops: HashMap<OperationId, WrappedOperation>,
    /// one vec per thread
    ops_by_thread_and_interest:
        Vec<BTreeSet<(std::cmp::Reverse<num::rational::Ratio<u64>>, OperationId)>>, // [thread][order by: (rev rentability, OperationId)]
    current_periods: Vec<u64>,
    cfg: ConsensusConfig,
}

impl OperationPool {
    pub fn new(current_periods: Vec<u64>, cfg: ConsensusConfig) -> OperationPool {
        OperationPool {
            ops: HashMap::new(),
            ops_by_thread_and_interest: vec![BTreeSet::new(); cfg.thread_count as usize],
            current_periods,
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
        let op_id = OperationId(Hash::hash(&operation.signature.to_bytes()));
        if self.ops.contains_key(&op_id) {
            return Ok(false);
        }

        let wrapped_op = WrappedOperation::new(operation, self.cfg.thread_count, context)?;
        let thread = wrapped_op.thread;
        let interest = (std::cmp::Reverse(wrapped_op.get_fee_density()), op_id);

        let signature_engine = SignatureEngine::new();
        let hash = wrapped_op.op.content.compute_hash(context)?;

        // period validity check
        if !wrapped_op.is_valid_at_period(
            self.current_periods[wrapped_op.thread as usize],
            self.cfg.operation_validity_periods,
        ) {
            return Ok(false);
        }
        signature_engine.verify(
            &hash,
            &wrapped_op.op.signature,
            &wrapped_op.op.content.creator_public_key,
        )?;

        self.ops_by_thread_and_interest[thread as usize].insert(interest);
        self.ops.insert(op_id, wrapped_op);
        // remove excess
        while self.ops_by_thread_and_interest[thread as usize].len()
            > self.cfg.max_operations_per_block as usize
        {
            // normalement 1 seule itération
            let (_removed_rentability, removed_id) = self.ops_by_thread_and_interest
                [thread as usize]
                .pop_last()
                .unwrap(); // will not panic because of the while condition. complexité = log ou mieux
            self.ops.remove(&removed_id); // complexité: const
        }

        Ok(true)
    }

    /// Update current_slot and discard invalid or integrated operation
    pub fn ack_final_block(&mut self, periods: Vec<u64>) -> Result<(), ConsensusError> {
        todo!()
    }

    /// Get max_count operation for thread block_slot.thread
    /// if vec is not full that means that there is no more interesting transactions left
    pub fn get_ops(
        &mut self,
        block_slot: Slot,
        exclude: HashSet<OperationId>,
        max_count: usize,
    ) -> Result<Vec<(OperationId, Operation)>, ConsensusError> {
        Ok(self.ops_by_thread_and_interest[block_slot.thread as usize]
            .iter()
            .filter_map(|(_rentability, id)| {
                if exclude.contains(id) {
                    return None;
                }
                if let Some(op) = self.ops.get(id) {
                    if op.is_valid_at_period(block_slot.period, self.cfg.operation_validity_periods)
                    {
                        Some((id.clone(), op.op.clone()))
                    } else {
                        None
                    }
                } else {
                    None // container inconsistency but don't know how to return the error from the filter
                }
            })
            .take(max_count)
            .collect::<Vec<(OperationId, Operation)>>())
    }
}

fn get_thread(key: PublicKey) -> u8 {
    todo!()
}
