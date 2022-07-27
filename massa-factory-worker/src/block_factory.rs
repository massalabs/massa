// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_factory_exports::{FactoryChannels, FactoryConfig};
use massa_hash::Hash;
use massa_models::{
    prehash::{Map, Set},
    timeslots::{get_block_slot_timestamp, get_closest_slot_to_timestamp},
    wrapped::WrappedContent,
    Address, Amount, Block, BlockHeader, BlockId, BlockSerializer, OperationId, Slot,
    WrappedEndorsement,
};
use massa_time::MassaTime;
use massa_wallet::Wallet;
use std::{
    sync::{mpsc, Arc, RwLock},
    thread,
    time::Instant,
};
use tracing::{info, warn};

/// Structure gathering all elements needed by the factory thread
pub(crate) struct BlockFactoryWorker {
    cfg: FactoryConfig,
    wallet: Arc<RwLock<Wallet>>,
    channels: FactoryChannels,
    factory_receiver: mpsc::Receiver<()>,
    block_serializer: BlockSerializer,
}

impl BlockFactoryWorker {
    /// Creates the `FactoryThread` structure to gather all data and references
    /// needed by the factory worker thread.
    pub(crate) fn spawn(
        cfg: FactoryConfig,
        wallet: Arc<RwLock<Wallet>>,
        channels: FactoryChannels,
        factory_receiver: mpsc::Receiver<()>,
    ) -> thread::JoinHandle<()> {
        thread::Builder::new()
            .name("block factory worker".into())
            .spawn(|| {
                let this = Self {
                    cfg,
                    wallet,
                    channels,
                    factory_receiver,
                    block_serializer: BlockSerializer::new(),
                };
                this.run();
            })
            .expect("could not spawn block factory worker thread")
    }

    /// Gets the next slot and the instant when it will happen.
    /// Slots can be skipped if we waited too much in-between.
    /// Extra safety against double-production caused by clock adjustments (this is the role of the previous_slot parameter).
    fn get_next_slot(&self, previous_slot: Option<Slot>) -> (Slot, Instant) {
        // get current absolute time
        let mut now = MassaTime::compensated_now(self.cfg.clock_compensation_millis)
            .expect("could not get current time");

        // if it's the first computed slot, add a time shift to prevent double-production on node restart with clock skew
        let base_time = if previous_slot.is_none() {
            now.saturating_add(self.cfg.initial_delay)
        } else {
            now
        };

        // get closest slot according to the current absolute time
        let mut next_slot = get_closest_slot_to_timestamp(
            self.cfg.thread_count,
            self.cfg.t0,
            self.cfg.genesis_timestamp,
            base_time,
        );

        // ignore genesis
        if next_slot.period == 0 {
            next_slot.period = 1;
        }

        // protection against double-production on unexpected system clock adjustment
        if let Some(prev_slot) = previous_slot {
            if next_slot <= prev_slot {
                next_slot = prev_slot
                    .get_next_slot(self.cfg.thread_count)
                    .expect("could not compute next slot");
            }
        }

        // get the tiemstamp of the target slot
        let next_instant = get_block_slot_timestamp(
            self.cfg.thread_count,
            self.cfg.t0,
            self.cfg.genesis_timestamp,
            next_slot,
        )
        .expect("could not get block slot timestamp")
        .estimate_instant(self.cfg.clock_compensation_millis)
        .expect("could not estimate block slot instant");

        (next_slot, next_instant)
    }

    /// Interruptibly wait until an instant or a stop signal
    ///
    /// # Return value
    /// Returns `true` if the instant was reached, otherwise `false` if there was an interruption.
    fn interruptible_wait_until(&self, duration: Instant) -> bool {
        loop {
            match self.factory_receiver.recv_deadline(duration) {
                // message received => quit main loop
                Ok(()) => return false,
                // timeout => continue main loop
                Err(mpsc::RecvTimeoutError::Timeout) => return true,
                // channel disconnected (sender dropped) => quit main loop
                Err(mpsc::RecvTimeoutError::Disconnected) => return false,
            }
        }
    }

    /// gather operations
    fn gather_operations(&self, slot: &Slot) -> (Vec<OperationId>, Hash) {
        // init list of selected operation IDs
        let mut op_ids = Vec::new();
        // init concatenated hashes for global operations hash computation
        let mut concatenated_hashes = Vec::new();
        // init remaining space
        let mut remaining_space = self.cfg.max_block_size;
        // init remaining gas
        let mut remaining_gas = self.cfg.max_block_gas;
        // cache of sequential balances
        let mut sequential_balance_cache: Map<Address, Amount> = Default::default();
        // list of excluded operation IDs
        let mut excluded_ops: Set<OperationId> =
            self.channels.execution.get_executed_ops(slot.thread);
        // production timeout instant
        let timeout_instant = Instant::now()
            .checked_add(self.cfg.production_timeout.to_duration())
            .expect("could not set production timeout instant");

        // loop over operations
        loop {
            // get operations
            let op_batch = self.channels.pool.get_operation_batch(
                slot,
                remaining_gas,
                remaining_space,
                &excluded_ops,
            );

            // stop if there are no more suitable operations in the pool
            if op_batch.is_empty() {
                break;
            }

            // get balance for uncached addresses in a single batch
            {
                let missing_balance_ids = op_batch
                    .iter()
                    .filter_map(|op_info| {
                        if !sequential_balance_cache.contains_key(&op_info.sender_id) {
                            return Some(op_info.sender_id);
                        }
                        None
                    })
                    .collect();
                let missing_balances = self
                    .channels
                    .execution
                    .get_sequential_balances(missing_balance_ids)
                    .into_iter()
                    .map(|sender_id, opt_balance| (sender_id, opt_balance.unwrap_or_default()))
                    .collect();
                sequential_balance_cache.extend(missing_balances);
            }

            // process ops
            for OperationInfo {
                id,
                size,
                max_gas,
                sender_address,
                fees,
                ..
            } in op_batch
            {
                // exclude from future batches
                excluded_ops.insert(id);

                // check remaining space
                if remaining_space < size {
                    continue;
                }

                // check remaining gas
                if remaining_gas < max_gas {
                    continue;
                }

                // check remaining sequential balance
                // won't panic because all missing balances from cache were filled above
                let seq_balance = sequential_balance_cache
                    .get_mut(&sender_address)
                    .expect("could not get sequential balance from cache for block creation");
                if seq_balance < fees {
                    continue;
                }

                // from here, we consider the operation as accepted

                // update remaining space
                remaining_space -= size;

                // update remaining gas
                remaining_gas -= max_gas;

                // update balance cache
                *seq_balance = seq_balance - fees;

                // add operation to list
                op_ids.push(id);

                // extend concatenated hashes
                concatenated_hashes.extend(id.to_bytes());
            }

            // if we took too much time, stop the loop
            if Instant::now() > timeout_instant {
                info!(
                    "operation gathering timeout reached for block creation at slot {}",
                    slot
                );
                break;
            }
        }

        // compute global operations hash
        let global_hash = Hash::compute_from(&concatenated_hashes);

        (op_ids, global_hash)
    }

    /// Process a slot: produce a block at that slot if one of the managed keys is drawn.
    fn process_slot(&mut self, slot: Slot) {
        // get block producer address for that slot
        let block_producer_addr = match self.channels.selector.get_producer(slot) {
            Ok(addr) => addr,
            Err(err) => {
                warn!(
                    "block factory could not get selector draws for slot {}: {}",
                    slot, err
                );
                return;
            }
        };

        // check if the block producer address is handled by the wallet
        let block_producer_keypair = match self
            .wallet
            .read()
            .expect("could not lock wallet")
            .find_associated_keypair(&block_producer_addr)
        {
            // the selected block producer is managed locally => continue to attempt block production
            Some(kp) => kp,
            // the selected block producer is not managed locally => quit
            None => return,
        };

        // get best parents and their periods
        let parents: Vec<(BlockId, u64)> = self.channels.consensus.get_best_parents(); // Vec<(parent_id, parent_period)>

        // get the parent in the same thread, with its period
        // will not panic because the thread is validated before the call
        let (same_thread_parent_id, same_thread_parent_period) = parents[slot.thread as usize];

        // gather endorsements
        // TODO make pool non-async
        // TODO change get_endorsements in pool so that it reads PoS by itself and takes parameters (endorsed_id, endorsed_slot)
        // TODO this should never fail
        let endorsements: Vec<Option<WrappedEndorsement>> = self.channels.pool.get_endorsements(
            same_thread_parent_id,
            Slot::new(same_thread_parent_period, slot.thread),
        );

        // gather operations
        let (operations, global_operations_hash) = self.gather_operations(&slot);

        // create header
        let header = BlockHeader::new_wrapped(
            BlockHeader {
                slot,
                parents: parents.into_iter().map(|(id, _period)| id).collect(),
                operation_merkle_root: global_operations_hash,
                endorsements,
            },
            Default::default(),
            block_producer_keypair,
        )
        .expect("error while producing block header");

        // create block
        let block = Block::new_wrapped(
            Block { header, operations },
            self.block_serializer,
            block_producer_keypair,
        )
        .expect("error while producing block");

        // TODO should we store block in storage here ?

        // log block creation
        info!(
            "block {} created at slot {} by address {}",
            block.id, slot, block.creator_address
        );

        // send block to consensus
        self.channels.consensus.send_block(block);
    }

    /// main run loop of the block creator thread
    fn run(self) {
        let mut prev_slot = None;
        loop {
            // get next slot
            let (slot, block_instant) = self.get_next_slot(prev_slot);

            // wait until slot
            if !self.interruptible_wait_until(block_instant) {
                break;
            }

            // process slot
            self.process_slot(slot);

            // update previous slot
            prev_slot = Some(slot);
        }
    }
}
