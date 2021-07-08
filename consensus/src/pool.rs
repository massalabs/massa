use std::{
    cmp::max,
    collections::{BTreeSet, HashMap, HashSet},
};

use crypto::{hash::Hash, signature::SignatureEngine};
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

    /// Used to compare operations
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
    /// latest final blocks periods
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
        // Already present
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
        // remove excess operation if pool is full
        while self.ops_by_thread_and_interest[thread as usize].len()
            > self.cfg.max_pool_size as usize
        {
            // should be one iteration
            let (_removed_rentability, removed_id) = self.ops_by_thread_and_interest
                [thread as usize]
                .pop_last()
                .unwrap(); // will not panic because of the while condition. complexity = log or better
            self.ops.remove(&removed_id); // complexity: const
        }

        Ok(true)
    }

    // removes an operation
    fn remove_op(&mut self, op_id: OperationId) {
        if let Some(wrapped_op) = self.ops.remove(&op_id) {
            // complexity: const
            let interest = (std::cmp::Reverse(wrapped_op.get_fee_density()), op_id);
            self.ops_by_thread_and_interest[wrapped_op.thread as usize].remove(&interest);
            // complexity: log
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
                        !op.is_valid_at_period(*period, self.cfg.operation_validity_periods)
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
                max_pool_size: 100000,
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
    fn test_pool() {
        let (mut cfg, context) = example_consensus_config();
        cfg.max_pool_size = 10;
        let periods = vec![0, 0]; // thread_count =2
        let mut pool = OperationPool::new(periods, cfg);

        // generate transactions
        // t0 -> t10 -> valids
        // t11 -> t17 -> not yet managed
        let mut vec = Vec::new();
        for i in 0..18 {
            let (op, thread) = get_transaction(40 + i, 40 + i, &context);
            let res = pool.new_operation(op.clone(), &context).unwrap();
            if i > 10 {
                // not yet valid
                assert!(!res);
            } else {
                // valid ops
                vec.push((op.clone(), thread));
                assert!(res);
            }
        }

        // t18 -> t21 -> duplicates
        let add = Vec::new();
        for (op, _thread) in vec[0..5].iter() {
            //add.push((op.clone(), thread.clone()));
            assert!(!pool.new_operation(op.clone(), &context).unwrap());
        }

        vec.extend(add.into_iter());

        let mut thread0: Vec<_> = vec
            .iter()
            .filter(|(_op, thread)| *thread == 0)
            .cloned()
            .collect();
        let mut thread1: Vec<_> = vec
            .iter()
            .filter(|(_op, thread)| *thread == 1)
            .cloned()
            .collect();

        // sort from bigger fee to smaller
        thread0.reverse();
        thread1.reverse();

        // checks ops for thread 0
        let res = pool.get_ops(Slot::new(1, 0), HashSet::new(), 50).unwrap();
        assert_eq!(res.len(), thread0.len());
        let res_op: Vec<_> = res.iter().map(|(_id, op)| op.clone()).collect();
        let thread0_op: Vec<_> = thread0.iter().map(|(op, _thread)| op.clone()).collect();
        assert_eq!(res_op, thread0_op);

        // checks ops for thread 1
        let res = pool.get_ops(Slot::new(1, 1), HashSet::new(), 50).unwrap();
        assert_eq!(res.len(), thread1.len());
        let res_op: Vec<_> = res.iter().map(|(_id, op)| op.clone()).collect();
        let thread1_op: Vec<_> = thread1.iter().map(|(op, _thread)| op.clone()).collect();
        assert_eq!(res_op, thread1_op);

        // op after before 45 should be discarded
        pool.ack_final_block(vec![45, 45]).unwrap();

        // Update expected
        let thread0: Vec<_> = thread0
            .iter()
            .filter(|(op, _thread)| op.content.expiration_period >= 45)
            .cloned()
            .collect();
        let thread1: Vec<_> = thread1
            .iter()
            .filter(|(op, _thread)| op.content.expiration_period >= 45)
            .cloned()
            .collect();

        // checks ops for thread 0
        let res = pool.get_ops(Slot::new(1, 0), HashSet::new(), 50).unwrap();
        assert_eq!(res.len(), thread0.len());
        let res_op: Vec<_> = res.iter().map(|(_id, op)| op.clone()).collect();
        let thread0_op: Vec<_> = thread0.iter().map(|(op, _thread)| op.clone()).collect();
        assert_eq!(res_op, thread0_op);

        // checks ops for thread 1
        let res = pool.get_ops(Slot::new(1, 1), HashSet::new(), 50).unwrap();
        assert_eq!(res.len(), thread1.len());
        let res_op: Vec<_> = res.iter().map(|(_id, op)| op.clone()).collect();
        let thread1_op: Vec<_> = thread1.iter().map(|(op, _thread)| op.clone()).collect();
        assert_eq!(res_op, thread1_op);
    }
}
