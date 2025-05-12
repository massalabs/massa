//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_channel::receiver::MassaReceiver;
use massa_factory_exports::{FactoryChannels, FactoryConfig};
use massa_models::{
    block::{Block, BlockSerializer},
    block_header::{BlockHeader, BlockHeaderSerializer, SecuredHeader},
    block_id::BlockId,
    endorsement::SecureShareEndorsement,
    operation::{compute_operations_hash, OperationIdSerializer},
    secure_share::SecureShareContent,
    slot::Slot,
    timeslots::{get_block_slot_timestamp, get_closest_slot_to_timestamp},
};
use massa_time::MassaTime;
use massa_versioning::versioning::MipStore;
use massa_wallet::Wallet;
use parking_lot::RwLock;
use std::{sync::Arc, thread, time::Instant};
use tracing::{info, warn};

/// Structure gathering all elements needed by the factory thread
pub(crate) struct BlockFactoryWorker {
    cfg: FactoryConfig,
    wallet: Arc<RwLock<Wallet>>,
    channels: FactoryChannels,
    factory_receiver: MassaReceiver<()>,
    mip_store: MipStore,
    op_id_serializer: OperationIdSerializer,
}

impl BlockFactoryWorker {
    /// Creates the `FactoryThread` structure to gather all data and references
    /// needed by the factory worker thread.
    pub(crate) fn spawn(
        cfg: FactoryConfig,
        wallet: Arc<RwLock<Wallet>>,
        channels: FactoryChannels,
        factory_receiver: MassaReceiver<()>,
        mip_store: MipStore,
    ) -> thread::JoinHandle<()> {
        thread::Builder::new()
            .name("block-factory".into())
            .spawn(|| {
                let mut this = Self {
                    cfg,
                    wallet,
                    channels,
                    factory_receiver,
                    mip_store,
                    op_id_serializer: OperationIdSerializer::new(),
                };
                this.run();
            })
            .expect("failed to spawn thread : block-factory")
    }

    /// Gets the next slot and the instant when it will happen.
    /// Slots can be skipped if we waited too much in-between.
    /// Extra safety against double-production caused by clock adjustments (this is the role of the `previous_slot` parameter).
    fn get_next_slot(&self, previous_slot: Option<Slot>) -> (Slot, Instant) {
        // get current absolute time
        let now = MassaTime::now();

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
        if next_slot.period <= self.cfg.last_start_period {
            next_slot = Slot::new(self.cfg.last_start_period + 1, 0);
        }

        // protection against double-production on unexpected system clock adjustment
        if let Some(prev_slot) = previous_slot {
            if next_slot <= prev_slot {
                next_slot = prev_slot
                    .get_next_slot(self.cfg.thread_count)
                    .expect("could not compute next slot");
            }
        }

        // get the timestamp of the target slot
        let next_instant = get_block_slot_timestamp(
            self.cfg.thread_count,
            self.cfg.t0,
            self.cfg.genesis_timestamp,
            next_slot,
        )
        .expect("could not get block slot timestamp")
        .estimate_instant()
        .expect("could not estimate block slot instant");

        (next_slot, next_instant)
    }

    /// Wait and interrupt or wait until an instant or a stop signal
    ///
    /// # Return value
    /// Returns `true` if the instant was reached, otherwise `false` if there was an interruption.
    fn interruptible_wait_until(&self, deadline: Instant) -> bool {
        match self.factory_receiver.recv_deadline(deadline) {
            // message received => quit main loop
            Ok(()) => false,
            // timeout => continue main loop
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => true,
            // channel disconnected (sender dropped) => quit main loop
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => false,
        }
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
        let block_producer_keypair_ref = self.wallet.read();
        let block_producer_keypair = if let Some(kp) =
            block_producer_keypair_ref.find_associated_keypair(&block_producer_addr)
        {
            // the selected block producer is managed locally => continue to attempt block production
            kp
        } else {
            // the selected block producer is not managed locally => quit
            return;
        };
        let mut block_storage = self.channels.storage.clone_without_refs();
        {
            let block_lock = block_storage.read_blocks();
            if let Some(block_ids) = block_lock.get_blocks_by_slot(&slot) {
                for block_id in block_ids {
                    if let Some(block) = block_lock.get(block_id) {
                        if block.content_creator_address == block_producer_addr {
                            panic!("You already created a block for slot {} with address {}, node is stopping to prevent you from losing all your stake due to double staking protection", slot, block_producer_addr);
                        }
                    }
                }
            }
        }

        // check if we need to have connections to produce a block and in this case, check if we have enough.
        #[cfg(not(feature = "sandbox"))]
        if self.cfg.stop_production_when_zero_connections {
            if let Ok(stats) = self.channels.protocol.get_stats() {
                if stats.1.is_empty() {
                    warn!("block factory could not produce block for slot {} because there are no connections", slot);
                    return;
                }
            }
        }

        // get best parents and their periods
        let parents: Vec<(BlockId, u64)> = self.channels.consensus.get_best_parents(); // Vec<(parent_id, parent_period)>
                                                                                       // generate the local storage object

        // get the parent in the same thread, with its period
        // will not panic because the thread is validated before the call
        let (same_thread_parent_id, _) = parents[slot.thread as usize];

        // gather endorsements
        let (endorsements_ids, endo_storage) = self
            .channels
            .pool
            .get_block_endorsements(&same_thread_parent_id, &slot);
        //TODO: Do we want to populate only with endorsement id in the future ?
        let endorsements: Vec<SecureShareEndorsement> = {
            let endo_read = endo_storage.read_endorsements();
            endorsements_ids
                .into_iter()
                .flatten()
                .map(|endo_id| {
                    endo_read
                        .get(&endo_id)
                        .expect("could not retrieve endorsement")
                        .clone()
                })
                .collect()
        };
        block_storage.extend(endo_storage);

        // gather operations and compute global operations hash
        let (op_ids, op_storage) = self.channels.pool.get_block_operations(&slot);
        if op_ids.len() > self.cfg.max_operations_per_block as usize {
            warn!("Too many operations returned");
            return;
        }

        block_storage.extend(op_storage);

        // create header
        let current_version = self.mip_store.get_network_version_current();
        let announced_version = self.mip_store.get_network_version_to_announce();
        let header: SecuredHeader = BlockHeader::new_verifiable::<BlockHeaderSerializer, BlockId>(
            BlockHeader {
                current_version,
                announced_version,
                slot,
                parents: parents.into_iter().map(|(id, _period)| id).collect(),
                operation_merkle_root: compute_operations_hash(&op_ids, &self.op_id_serializer),
                endorsements,
                denunciations: self.channels.pool.get_block_denunciations(&slot),
            },
            BlockHeaderSerializer::new(), // TODO reuse self.block_header_serializer
            block_producer_keypair,
            self.cfg.chain_id,
        )
        .expect("error while producing block header");
        // create block
        let block_ = Block {
            header,
            operations: op_ids.into_iter().collect(),
        };

        let block = Block::new_verifiable(
            block_,
            BlockSerializer::new(), // TODO reuse self.block_serializer
            block_producer_keypair,
            self.cfg.chain_id,
        )
        .expect("error while producing block");
        let block_id = block.id;
        // store block in storage
        block_storage.store_block(block);

        // log block creation
        info!(
            "block {} created at slot {} by address {}",
            block_id, slot, block_producer_addr
        );

        // send full block to consensus
        self.channels
            .consensus
            .register_block(block_id, slot, block_storage, true);
    }

    /// main run loop of the block creator thread
    fn run(&mut self) {
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
