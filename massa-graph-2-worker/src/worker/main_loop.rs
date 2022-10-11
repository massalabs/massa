use std::{collections::HashMap, mem, sync::mpsc, time::Instant};

use massa_graph::error::GraphResult;
use massa_graph_2_exports::block_status::BlockStatus;
use massa_logging::massa_trace;
use massa_models::{
    block::{BlockId, WrappedHeader},
    prehash::{PreHashMap, PreHashSet},
    slot::Slot,
    timeslots::{get_block_slot_timestamp, get_closest_slot_to_timestamp},
};
use massa_storage::Storage;
use massa_time::MassaTime;
use tracing::log::warn;

use crate::commands::GraphCommand;

use super::GraphWorker;

enum WaitingStatus {
    Ended,
    Interrupted,
    Disconnected,
}

impl GraphWorker {
    fn manage_command(&mut self, command: GraphCommand) -> GraphResult<()> {
        match command {
            GraphCommand::RegisterBlockHeader(block_id, header) => {
                {
                    let mut write_shared_state = self.shared_state.write();
                    write_shared_state.register_block_header(
                        block_id,
                        header,
                        self.previous_slot,
                    )?;
                }
                self.block_db_changed()
            }
            GraphCommand::RegisterBlock(block_id, slot, block_storage) => {
                {
                    let mut write_shared_state = self.shared_state.write();
                    write_shared_state.register_block(
                        block_id,
                        slot,
                        self.previous_slot,
                        block_storage,
                    )?;
                }
                self.block_db_changed()
            }
            _ => {
                Ok(())
                // TODO
            }
        }
    }

    /// Wait and interrupt or wait until an instant or a stop signal
    ///
    /// # Return value
    /// Returns the error of the process of the command if any.
    /// Returns true if we reached the instant.
    /// Returns false if we were interrupted by a command.
    fn wait_slot_or_command(&mut self, deadline: Instant) -> WaitingStatus {
        match self.command_receiver.recv_deadline(deadline) {
            // message received => manage it
            Ok(command) => {
                if let Err(err) = self.manage_command(command) {
                    warn!("Error in graph: {}", err);
                }
                WaitingStatus::Interrupted
            }
            // timeout => continue main loop
            Err(mpsc::RecvTimeoutError::Timeout) => WaitingStatus::Ended,
            // channel disconnected (sender dropped) => quit main loop
            Err(mpsc::RecvTimeoutError::Disconnected) => WaitingStatus::Disconnected,
        }
    }

    /// Gets the next slot and the instant when it will happen.
    /// Slots can be skipped if we waited too much in-between.
    /// Extra safety against double-production caused by clock adjustments (this is the role of the `previous_slot` parameter).
    fn get_next_slot(&self, previous_slot: Option<Slot>) -> (Slot, Instant) {
        // get current absolute time
        let now = MassaTime::now(self.config.clock_compensation_millis)
            .expect("could not get current time");

        // get closest slot according to the current absolute time
        let mut next_slot = get_closest_slot_to_timestamp(
            self.config.thread_count,
            self.config.t0,
            self.config.genesis_timestamp,
            now,
        );

        // protection against double-production on unexpected system clock adjustment
        if let Some(prev_slot) = previous_slot {
            if next_slot <= prev_slot {
                next_slot = prev_slot
                    .get_next_slot(self.config.thread_count)
                    .expect("could not compute next slot");
            }
        }

        // get the timestamp of the target slot
        let next_instant = get_block_slot_timestamp(
            self.config.thread_count,
            self.config.t0,
            self.config.genesis_timestamp,
            next_slot,
        )
        .expect("could not get block slot timestamp")
        .estimate_instant(self.config.clock_compensation_millis)
        .expect("could not estimate block slot instant");

        (next_slot, next_instant)
    }

    /// Notify execution about blockclique changes and finalized blocks.
    ///
    /// # Arguments:
    /// * `finalized_blocks`: Block that became final and need to be send to execution
    fn notify_execution(&mut self, finalized_blocks: HashMap<Slot, BlockId>) {
        let read_shared_state = self.shared_state.read();
        // List new block storage instances that Execution doesn't know about.
        // That's blocks that have not been sent to execution before, ie. in the previous blockclique).
        let mut new_blocks_storage: PreHashMap<BlockId, Storage> = finalized_blocks
            .iter()
            .filter_map(|(_slot, b_id)| {
                if self.prev_blockclique.contains_key(b_id) {
                    // was previously sent as a blockclique element
                    return None;
                }
                let storage = match read_shared_state.block_statuses.get(b_id) {
                    Some(BlockStatus::Active {
                        a_block: _,
                        storage,
                    }) => storage,
                    _ => panic!("final block not found in active blocks"),
                };
                Some((*b_id, storage.clone()))
            })
            .collect();

        // Get new blockclique block list with slots.
        let mut blockclique_changed = false;
        let new_blockclique: PreHashMap<BlockId, Slot> = read_shared_state
            .get_blockclique()
            .iter()
            .map(|b_id| {
                if let Some(slot) = self.prev_blockclique.remove(b_id) {
                    // The block was already sent in the previous blockclique:
                    // the slot can be gathered from there without locking Storage.
                    // Note: the block is removed from self.prev_blockclique.
                    (*b_id, slot)
                } else {
                    // The block was not present in the previous blockclique:
                    // the blockclique has changed => get the block's slot by querying Storage.
                    blockclique_changed = true;
                    let (slot, storage) = match read_shared_state.block_statuses.get(b_id) {
                        Some(BlockStatus::Active { a_block, storage }) => (a_block.slot, storage),
                        _ => panic!("blockclique block not found in active blocks"),
                    };
                    new_blocks_storage.insert(*b_id, storage.clone());
                    (*b_id, slot)
                }
            })
            .collect();
        if !self.prev_blockclique.is_empty() {
            // All elements present in the new blockclique have been removed from `prev_blockclique` above.
            // If `prev_blockclique` is not empty here, it means that it contained elements that are not in the new blockclique anymore.
            // In that case, we mark the blockclique as having changed.
            blockclique_changed = true;
        }
        // Overwrite previous blockclique.
        // Should still be done even if unchanged because elements were removed from it above.
        self.prev_blockclique = new_blockclique.clone();

        if finalized_blocks.is_empty() && !blockclique_changed {
            // There are no changes (neither block finalizations not blockclique changes) to send to execution.
            return;
        }

        // Notify execution of block finalizations and blockclique changes
        self.channels
            .execution_controller
            .update_blockclique_status(
                finalized_blocks,
                if blockclique_changed {
                    Some(new_blockclique.into_iter().map(|(k, v)| (v, k)).collect())
                } else {
                    None
                },
                new_blocks_storage,
            );
    }

    /// call me if the block database changed
    /// Processing of final blocks, pruning.
    ///
    /// 1. propagate blocks
    /// 2. Notify of attack attempts
    /// 3. get new final blocks
    /// 4. get blockclique
    /// 5. notify Execution
    /// 6. Process new final blocks
    /// 7. Notify pool of new final ops
    /// 8. Notify PoS of final blocks
    /// 9. notify protocol of block wish list
    /// 10. note new latest final periods (prune graph if changed)
    /// 11. add stale blocks to stats
    pub fn block_db_changed(&mut self) -> GraphResult<()> {
        let final_block_slots = {
            let mut write_shared_state = self.shared_state.write();
            massa_trace!("consensus.consensus_worker.block_db_changed", {});

            // Propagate new blocks
            for (block_id, storage) in mem::take(&mut write_shared_state.to_propagate).into_iter() {
                massa_trace!("consensus.consensus_worker.block_db_changed.integrated", {
                    "block_id": block_id
                });
                self.channels
                    .protocol_command_sender
                    .integrated_block(block_id, storage)?;
            }

            // Notify protocol of attack attempts.
            for hash in mem::take(&mut write_shared_state.attack_attempts).into_iter() {
                self.channels
                    .protocol_command_sender
                    .notify_block_attack(hash)?;
                massa_trace!("consensus.consensus_worker.block_db_changed.attack", {
                    "hash": hash
                });
            }

            // manage finalized blocks
            let timestamp = MassaTime::now(self.config.clock_compensation_millis)?;
            let finalized_blocks = mem::take(&mut write_shared_state.new_final_blocks);
            let mut final_block_slots = HashMap::with_capacity(finalized_blocks.len());
            for b_id in finalized_blocks {
                if let Some(BlockStatus::Active {
                    a_block,
                    storage: _,
                }) = write_shared_state.block_statuses.get(&b_id)
                {
                    // add to final blocks to notify execution
                    final_block_slots.insert(a_block.slot, b_id);

                    // add to stats
                    let block_is_from_protocol = self
                        .protocol_blocks
                        .iter()
                        .any(|(_, block_id)| block_id == &b_id);
                    self.final_block_stats.push_back((
                        timestamp,
                        a_block.creator_address,
                        block_is_from_protocol,
                    ));
                }
            }

            // add stale blocks to stats
            let new_stale_block_ids_creators_slots =
                mem::take(&mut write_shared_state.new_stale_blocks);
            let timestamp = MassaTime::now(self.config.clock_compensation_millis)?;
            for (_b_id, (_b_creator, _b_slot)) in new_stale_block_ids_creators_slots.into_iter() {
                self.stale_block_stats.push_back(timestamp);
            }
            final_block_slots
        };

        // notify execution
        self.notify_execution(final_block_slots);

        // notify protocol of block wishlist
        {
            let read_shared_state = self.shared_state.read();
            let new_wishlist = read_shared_state.get_block_wishlist()?;
            let new_blocks: PreHashMap<BlockId, Option<WrappedHeader>> = new_wishlist
                .iter()
                .filter_map(|(id, header)| {
                    if !self.wishlist.contains_key(id) {
                        Some((*id, header.clone()))
                    } else {
                        None
                    }
                })
                .collect();
            let remove_blocks: PreHashSet<BlockId> = self
                .wishlist
                .iter()
                .filter_map(|(id, _)| {
                    if !new_wishlist.contains_key(id) {
                        Some(*id)
                    } else {
                        None
                    }
                })
                .collect();
            if !new_blocks.is_empty() || !remove_blocks.is_empty() {
                massa_trace!("consensus.consensus_worker.block_db_changed.send_wishlist_delta", { "new": new_wishlist, "remove": remove_blocks });
                self.channels
                    .protocol_command_sender
                    .send_wishlist_delta(new_blocks, remove_blocks)?;
                self.wishlist = new_wishlist;
            }

            // note new latest final periods
            let latest_final_periods: Vec<u64> = read_shared_state
                .latest_final_blocks_periods
                .iter()
                .map(|(_block_id, period)| *period)
                .collect();
            // if changed...
            if self.latest_final_periods != latest_final_periods {
                // signal new last final periods to pool
                self.channels
                    .pool_command_sender
                    .notify_final_cs_periods(&latest_final_periods);
                // update final periods
                self.latest_final_periods = latest_final_periods;
            }
        };

        /*
        TODO add this again
        let creator_addr = Address::from_public_key(&b_creator);
        if self.staking_keys.contains_key(&creator_addr) {
            warn!("block {} that was produced by our address {} at slot {} became stale. This is probably due to a temporary desynchronization.", b_id, creator_addr, b_slot);
        }
        */

        Ok(())
    }

    /// Runs in loop forever. This loop must stop every slot to perform operations on stats and graph
    /// but can be stopped anytime by a command received.
    pub fn run(&mut self) {
        loop {
            match self.wait_slot_or_command(self.next_instant) {
                WaitingStatus::Ended => {
                    self.previous_slot = Some(self.next_slot);
                    if let Err(err) = self.slot_tick(self.next_slot) {
                        warn!("Error while processing block tick: {}", err);
                    }
                    if let Err(err) = self.stats_tick() {
                        warn!("Error while processing stats tick: {}", err);
                    }
                    (self.next_slot, self.next_instant) = self.get_next_slot(Some(self.next_slot));
                }
                WaitingStatus::Disconnected => {
                    break;
                }
                WaitingStatus::Interrupted => {
                    continue;
                }
            };
        }
    }
}
