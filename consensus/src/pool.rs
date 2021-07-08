use std::{
    cmp::max,
    collections::{BTreeSet, HashMap, HashSet},
};

use crypto::{hash::Hash, signature::PublicKey, signature::SignatureEngine};
use models::{Address, Operation, SerializationContext, SerializeCompact, Slot};
use num::rational::Ratio;

use crate::{ConsensusConfig, ConsensusError};

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
        let start = max(
            0,
            self.op.content.expiration_period as i64 - operation_validity_periods as i64,
        ) as u64;
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

    // remove an operation
    fn remove_op(&mut self, op_id: OperationId) {
        if let Some(wrapped_op) = self.ops.remove(&op_id) {
            // complexité: const
            let interest = (std::cmp::Reverse(wrapped_op.get_fee_density()), op_id);
            self.ops_by_thread_and_interest[wrapped_op.thread as usize].remove(&interest);
            // complexité: log
        }
    }

    /// Update current_slot and discard invalid or integrated operation
    pub fn ack_final_block(&mut self, periods: Vec<u64>) -> Result<(), ConsensusError> {
        for (thread, period) in periods.iter().enumerate() {
            if self.current_periods[thread] != *period {
                // if update is needed
                let to_remove: Vec<_> = self
                    .ops
                    .iter()
                    .filter(|(_id, op)| {
                        op.is_valid_at_period(*period, self.cfg.operation_validity_periods)
                    })
                    .map(|(id, _op)| id.clone())
                    .collect();
                for id in to_remove.iter() {
                    self.remove_op(*id);
                }
            }
        }
        Ok(())
    }

    /// Get max_count operation for thread block_slot.thread
    /// if vec is not full that means that there is no more interesting transactions left
    pub fn get_ops(
        &mut self,
        block_slot: Slot,
        exclude: HashSet<OperationId>,
        max_count: usize,
    ) -> Result<Vec<(OperationId, Operation)>, ConsensusError> {
        self.ops_by_thread_and_interest[block_slot.thread as usize]
            .iter()
            .filter_map(|(_rentability, id)| {
                if exclude.contains(id) {
                    return None;
                }
                if let Some(w_op) = self.ops.get(id) {
                    if !w_op.is_valid_at_period(block_slot.period, self.cfg.operation_validity_periods) {
                        return None;
                    }
                    Some(Ok((id.clone(), w_op.op.clone())))
                } else {
                    Some(Err(ConsensusError::ContainerInconsistency(
                        format!("operation pool get_ops inconsistency: op_id={:?} is in ops_by_thread_and_interest but not in ops", id)
                    )))
                }
            })
            .take(max_count)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use models::{Operation, OperationContent, OperationType};
    use time::UTime;

    fn example_consensus_config() -> (ConsensusConfig, SerializationContext) {
        let secp = SignatureEngine::new();
        let genesis_key = SignatureEngine::generate_random_private_key();
        let mut nodes = Vec::new();
        for _ in 0..2 {
            let private_key = SignatureEngine::generate_random_private_key();
            let public_key = secp.derive_public_key(&private_key);
            nodes.push((public_key, private_key));
        }
        let thread_count: u8 = 2;
        let max_block_size = 1024 * 1024;
        let max_operations_per_block = 1024;
        (
            ConsensusConfig {
                genesis_timestamp: UTime::now(0).unwrap(),
                thread_count,
                t0: 32.into(),
                selection_rng_seed: 42,
                genesis_key,
                nodes,
                current_node_index: 0,
                max_discarded_blocks: 10,
                future_block_processing_max_periods: 3,
                max_future_processing_blocks: 10,
                max_dependency_blocks: 10,
                delta_f0: 5,
                disable_block_creation: true,
                max_block_size,
                max_operations_per_block,
                operation_validity_periods: 50,
            },
            SerializationContext {
                max_block_size,
                max_block_operations: max_operations_per_block,
                parent_count: thread_count,
                max_peer_list_length: 128,
                max_message_size: 3 * 1024 * 1024,
                max_bootstrap_blocks: 100,
                max_bootstrap_cliques: 100,
                max_bootstrap_deps: 100,
                max_bootstrap_children: 100,
                max_ask_blocks_per_message: 10,
                max_bootstrap_message_size: 100000000,
            },
        )
    }

    fn get_transaction(
        expiration_period: u64,
        fee: u64,
        context: &SerializationContext,
    ) -> (Operation, u8) {
        let secp = SignatureEngine::new();
        let sender_priv = SignatureEngine::generate_random_private_key();
        let sender_pub = secp.derive_public_key(&sender_priv);

        let recv_priv = SignatureEngine::generate_random_private_key();
        let recv_pub = secp.derive_public_key(&recv_priv);

        let op = OperationType::Transaction {
            recipient_address: Address::new(recv_pub),
            amount: 0,
        };
        let content = OperationContent {
            fee,
            expiration_period,
            creator_public_key: sender_pub,
            op,
        };
        let hash = content.compute_hash(context).unwrap();
        let signature = secp.sign(&hash, &sender_priv).unwrap();

        (
            Operation { content, signature },
            Address::new(sender_pub).get_thread(2),
        )
    }

    #[test]
    fn test_new_operation() {
        let (cfg, context) = example_consensus_config();
        let periods = vec![0, 0]; // thread_count =2
        let mut pool = OperationPool::new(periods, cfg);
        let (t1, thread) = get_transaction(25, 10, &context);
        pool.new_operation(t1.clone(), &context).unwrap();

        let res = pool
            .get_ops(Slot::new(1, thread), HashSet::new(), 50)
            .unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].1, t1);
    }
}
