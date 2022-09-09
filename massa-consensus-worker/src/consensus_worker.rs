// Copyright (c) 2022 MASSA LABS <info@massa.net>
use massa_consensus_exports::{
    commands::ConsensusCommand,
    error::{ConsensusError, ConsensusResult as Result},
    settings::ConsensusWorkerChannels,
    ConsensusConfig,
};
use massa_graph::{BlockGraph, BlockGraphExport};
use massa_models::timeslots::{get_block_slot_timestamp, get_latest_block_slot_at_timestamp};
use massa_models::{address::Address, block::BlockId, slot::Slot};
use massa_models::{block::WrappedHeader, prehash::PreHashMap};
use massa_models::{prehash::PreHashSet, stats::ConsensusStats};
use massa_protocol_exports::{ProtocolEvent, ProtocolEventReceiver};
use massa_time::MassaTime;
use std::{cmp::max, collections::VecDeque};
use tokio::time::{sleep, sleep_until, Sleep};
use tracing::{info, warn};

/// Manages consensus.
pub struct ConsensusWorker {
    /// Consensus Configuration
    cfg: ConsensusConfig,
    /// Associated channels, sender and receivers
    channels: ConsensusWorkerChannels,
    /// Database containing all information about blocks, the `BlockGraph` and cliques.
    block_db: BlockGraph,
    /// Previous slot.
    previous_slot: Option<Slot>,
    /// Next slot
    next_slot: Slot,
    /// blocks we want
    wishlist: PreHashMap<BlockId, Option<WrappedHeader>>,
    /// latest final periods
    latest_final_periods: Vec<u64>,
    /// clock compensation
    clock_compensation: i64,
    /// Final block stats `(time, creator)`
    final_block_stats: VecDeque<(MassaTime, Address)>,
    /// Stale block timestamps
    stale_block_stats: VecDeque<MassaTime>,
    /// the time span considered for stats
    stats_history_timespan: MassaTime,
    /// the time span considered for desynchronization detection
    #[allow(dead_code)]
    stats_desync_detection_timespan: MassaTime,
    /// time at which the node was launched (used for desynchronization detection)
    launch_time: MassaTime,
}

impl ConsensusWorker {
    /// Creates a new consensus controller.
    /// Initiates the random selector.
    ///
    /// # Arguments
    /// * `cfg`: consensus configuration.
    /// * `protocol_command_sender`: associated protocol controller
    /// * `block_db`: Database containing all information about blocks, the blockgraph and cliques.
    /// * `controller_command_rx`: Channel receiving consensus commands.
    /// * `controller_event_tx`: Channel sending out consensus events.
    /// * `controller_manager_rx`: Channel receiving consensus management commands.
    pub(crate) async fn new(
        cfg: ConsensusConfig,
        channels: ConsensusWorkerChannels,
        block_db: BlockGraph,
        clock_compensation: i64,
    ) -> Result<ConsensusWorker> {
        let now = MassaTime::now(clock_compensation)?;
        let previous_slot = get_latest_block_slot_at_timestamp(
            cfg.thread_count,
            cfg.t0,
            cfg.genesis_timestamp,
            now,
        )?;
        let next_slot = previous_slot.map_or(Ok(Slot::new(0u64, 0u8)), |s| {
            s.get_next_slot(cfg.thread_count)
        })?;
        let latest_final_periods: Vec<u64> = block_db
            .get_latest_final_blocks_periods()
            .iter()
            .map(|(_block_id, period)| *period)
            .collect();
        info!(
            "Started node at time {}, cycle {}, period {}, thread {}",
            now.to_utc_string(),
            next_slot.get_cycle(cfg.periods_per_cycle),
            next_slot.period,
            next_slot.thread,
        );
        if cfg.genesis_timestamp > now {
            let (days, hours, mins, secs) = cfg
                .genesis_timestamp
                .saturating_sub(now)
                .days_hours_mins_secs()?;
            info!(
                "{} days, {} hours, {} minutes, {} seconds remaining to genesis",
                days, hours, mins, secs,
            )
        }
        massa_trace!("consensus.consensus_worker.new", {});

        // add genesis blocks to stats
        let genesis_addr = Address::from_public_key(&cfg.genesis_key.get_public_key());
        let mut final_block_stats = VecDeque::new();
        for thread in 0..cfg.thread_count {
            final_block_stats.push_back((
                get_block_slot_timestamp(
                    cfg.thread_count,
                    cfg.t0,
                    cfg.genesis_timestamp,
                    Slot::new(0, thread),
                )?,
                genesis_addr,
            ))
        }

        // desync detection timespan
        let stats_desync_detection_timespan = cfg.t0.checked_mul(cfg.periods_per_cycle * 2)?;

        // notify execution module of current blockclique and final blocks
        // we need to do this because the bootstrap snapshots of the executor vs the consensus may not have been taken in sync
        // because the two modules run concurrently and out of sync

        let final_blocks = block_db.get_all_final_blocks();

        let blockclique = block_db
            .get_blockclique()
            .into_iter()
            .map(|block_id| {
                let (a_block, storage) = block_db
                    .get_active_block(&block_id)
                    .expect("could not get active block for execution notification");
                (a_block.slot, (block_id, storage.clone()))
            })
            .collect();
        channels
            .execution_controller
            .update_blockclique_status(final_blocks, blockclique);

        Ok(ConsensusWorker {
            block_db,
            previous_slot,
            next_slot,
            wishlist: Default::default(),
            latest_final_periods,
            clock_compensation,
            channels,
            final_block_stats,
            stale_block_stats: VecDeque::new(),
            stats_desync_detection_timespan,
            stats_history_timespan: max(stats_desync_detection_timespan, cfg.stats_timespan),
            cfg,
            launch_time: MassaTime::now(clock_compensation)?,
        })
    }

    /// Consensus work is managed here.
    /// It's mostly a tokio::select within a loop.
    pub async fn run_loop(mut self) -> Result<ProtocolEventReceiver> {
        // signal initial state to pool
        self.channels
            .pool_command_sender
            .notify_final_cs_periods(&self.latest_final_periods);

        // set slot timer
        let slot_deadline = get_block_slot_timestamp(
            self.cfg.thread_count,
            self.cfg.t0,
            self.cfg.genesis_timestamp,
            self.next_slot,
        )?
        .estimate_instant(self.clock_compensation)?;
        let next_slot_timer = sleep_until(tokio::time::Instant::from(slot_deadline));

        tokio::pin!(next_slot_timer);

        // set prune timer
        let prune_timer = sleep(self.cfg.block_db_prune_interval.to_duration());
        tokio::pin!(prune_timer);

        loop {
            massa_trace!("consensus.consensus_worker.run_loop.select", {});
            /*
                select! without the "biased" modifier will randomly select the 1st branch to check,
                then will check the next ones in the order they are written.
                We choose this order:
                    * manager commands: low freq, avoid having to wait to stop
                    * consensus commands (low to medium freq): respond quickly
                    * slot timer (low freq, timing is important but does not have to be perfect either)
                    * prune timer: low freq, timing not important but should not wait too long
                    * receive protocol events (high freq)
            */
            tokio::select! {
                // listen to manager commands
                cmd = self.channels.controller_manager_rx.recv() => {
                    massa_trace!("consensus.consensus_worker.run_loop.select.manager", {});
                    match cmd {
                    None => break,
                    Some(_) => {}
                }}

                // listen consensus commands
                Some(cmd) = self.channels.controller_command_rx.recv() => {
                    massa_trace!("consensus.consensus_worker.run_loop.consensus_command", {});
                    self.process_consensus_command(cmd).await?
                },

                // slot timer
                _ = &mut next_slot_timer => {
                    massa_trace!("consensus.consensus_worker.run_loop.select.slot_tick", {});
                    if let Some(end) = self.cfg.end_timestamp {
                        if MassaTime::now(self.clock_compensation)? > end {
                            info!("This episode has come to an end, please get the latest testnet node version to continue");
                            break;
                        }
                    }
                    self.slot_tick(&mut next_slot_timer).await?;
                },

                // prune timer
                _ = &mut prune_timer=> {
                    massa_trace!("consensus.consensus_worker.run_loop.prune_timer", {});
                    // prune block db
                    let _discarded_final_blocks = self.block_db.prune()?;

                    // reset timer
                    prune_timer.set(sleep( self.cfg.block_db_prune_interval.to_duration()))
                }

                // receive protocol controller events
                evt = self.channels.protocol_event_receiver.wait_event() =>{
                    massa_trace!("consensus.consensus_worker.run_loop.select.protocol_event", {});
                    match evt {
                        Ok(event) => self.process_protocol_event(event).await?,
                        Err(err) => return Err(ConsensusError::ProtocolError(Box::new(err)))
                    }
                },
            }
        }
        // after this curly brace you can find the end of the loop
        Ok(self.channels.protocol_event_receiver)
    }

    /// this function is called around every slot tick
    /// it checks for cycle increment
    /// creates block and endorsement if a staking address has been drawn
    /// it signals the new slot to other components
    /// detects desynchronization
    /// produce quite more logs than actual stuff
    async fn slot_tick(&mut self, next_slot_timer: &mut std::pin::Pin<&mut Sleep>) -> Result<()> {
        let now = MassaTime::now(self.clock_compensation)?;
        let observed_slot = get_latest_block_slot_at_timestamp(
            self.cfg.thread_count,
            self.cfg.t0,
            self.cfg.genesis_timestamp,
            now,
        )?;

        if observed_slot < Some(self.next_slot) {
            // reset timer for next slot
            let sleep_deadline = get_block_slot_timestamp(
                self.cfg.thread_count,
                self.cfg.t0,
                self.cfg.genesis_timestamp,
                self.next_slot,
            )?
            .estimate_instant(self.clock_compensation)?;
            next_slot_timer.set(sleep_until(tokio::time::Instant::from(sleep_deadline)));
            return Ok(());
        }

        let observed_slot = observed_slot.unwrap(); // does not panic, checked above

        massa_trace!("consensus.consensus_worker.slot_tick", {
            "slot": observed_slot
        });

        let previous_cycle = self
            .previous_slot
            .map(|s| s.get_cycle(self.cfg.periods_per_cycle));
        let observed_cycle = observed_slot.get_cycle(self.cfg.periods_per_cycle);
        if previous_cycle.is_none() {
            // first cycle observed
            info!("Massa network has started ! 🎉")
        }
        if previous_cycle < Some(observed_cycle) {
            info!("Started cycle {}", observed_cycle);
        }

        // check if there are any final blocks not produced by us
        // if none => we are probably desync
        /*
        TODO put this back
        #[cfg(not(feature = "sandbox"))]
        if now
            > max(self.cfg.genesis_timestamp, self.launch_time)
                .saturating_add(self.stats_desync_detection_timespan)
            && !self.final_block_stats.iter().any(|(time, _, addr)| {
                time > &now.saturating_sub(self.stats_desync_detection_timespan)
                    && !self.staking_keys.contains_key(addr)
            })
        {
            warn!("desynchronization detected because the recent final block history is empty or contains only blocks produced by this node");
            let _ = self.send_consensus_event(ConsensusEvent::NeedSync).await;
        }
        */

        self.previous_slot = Some(observed_slot);
        self.next_slot = observed_slot.get_next_slot(self.cfg.thread_count)?;

        // signal tick to block graph
        self.block_db.slot_tick(Some(observed_slot))?;

        // take care of block db changes
        self.block_db_changed().await?;

        // reset timer for next slot
        let sleep_deadline = get_block_slot_timestamp(
            self.cfg.thread_count,
            self.cfg.t0,
            self.cfg.genesis_timestamp,
            self.next_slot,
        )?
        .estimate_instant(self.clock_compensation)?;
        next_slot_timer.set(sleep_until(tokio::time::Instant::from(sleep_deadline)));

        // prune stats
        self.prune_stats()?;

        Ok(())
    }

    /// Manages given consensus command.
    /// They can come from the API or the bootstrap server
    /// Please refactor me
    ///
    /// # Argument
    /// * `cmd`: consensus command to process
    async fn process_consensus_command(&mut self, cmd: ConsensusCommand) -> Result<()> {
        match cmd {
            ConsensusCommand::GetBlockGraphStatus {
                slot_start,
                slot_end,
                response_tx,
            } => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_block_graph_status",
                    {}
                );
                if response_tx
                    .send(BlockGraphExport::extract_from(
                        &self.block_db,
                        slot_start,
                        slot_end,
                    )?)
                    .is_err()
                {
                    warn!("consensus: could not send GetBlockGraphStatus answer");
                }
                Ok(())
            }
            // gets the graph status of a batch of blocks
            ConsensusCommand::GetBlockStatuses { ids, response_tx } => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_block_statuses",
                    {}
                );
                let res: Vec<_> = ids
                    .iter()
                    .map(|id| self.block_db.get_block_status(id))
                    .collect();
                if response_tx.send(res).is_err() {
                    warn!("consensus: could not send get_block_statuses answer");
                }
                Ok(())
            }
            ConsensusCommand::GetCliques(response_tx) => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_cliques",
                    {}
                );
                if response_tx.send(self.block_db.get_cliques()).is_err() {
                    warn!("consensus: could not send GetSelectionDraws response");
                }
                Ok(())
            }
            ConsensusCommand::GetBootstrapState(response_tx) => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_bootstrap_state",
                    {}
                );
                let resp = self.block_db.export_bootstrap_graph()?;
                if response_tx.send(Box::new(resp)).await.is_err() {
                    warn!("consensus: could not send GetBootstrapState answer");
                }
                Ok(())
            }
            ConsensusCommand::GetStats(response_tx) => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_stats",
                    {}
                );
                let res = self.get_stats()?;
                if response_tx.send(res).is_err() {
                    warn!("consensus: could not send get_stats response");
                }
                Ok(())
            }
            ConsensusCommand::GetBestParents { response_tx } => {
                if response_tx
                    .send(self.block_db.get_best_parents().clone())
                    .is_err()
                {
                    warn!("consensus: could not send get best parents response");
                }
                Ok(())
            }
            ConsensusCommand::GetBlockcliqueBlockAtSlot { slot, response_tx } => {
                let res = self.block_db.get_blockclique_block_at_slot(&slot);
                if response_tx.send(res).is_err() {
                    warn!("consensus: could not send get block clique block at slot response");
                }
                Ok(())
            }
            ConsensusCommand::SendBlock {
                block_id,
                slot,
                block_storage,
                response_tx,
            } => {
                self.block_db
                    .incoming_block(block_id, slot, self.previous_slot, block_storage)?;

                if response_tx.send(()).is_err() {
                    warn!("consensus: could not send get block clique block at slot response");
                }
                Ok(())
            }
        }
    }

    /// retrieve stats
    /// Used in response to a API request
    fn get_stats(&mut self) -> Result<ConsensusStats> {
        let timespan_end = max(self.launch_time, MassaTime::now(self.clock_compensation)?);
        let timespan_start = max(
            timespan_end.saturating_sub(self.cfg.stats_timespan),
            self.launch_time,
        );
        let final_block_count = self
            .final_block_stats
            .iter()
            .filter(|(t, _)| *t >= timespan_start && *t < timespan_end)
            .count() as u64;
        let stale_block_count = self
            .stale_block_stats
            .iter()
            .filter(|t| **t >= timespan_start && **t < timespan_end)
            .count() as u64;
        let clique_count = self.block_db.get_clique_count() as u64;
        Ok(ConsensusStats {
            final_block_count,
            stale_block_count,
            clique_count,
            start_timespan: timespan_start,
            end_timespan: timespan_end,
        })
    }

    /// Manages received protocol events.
    ///
    /// # Arguments
    /// * `event`: event type to process.
    async fn process_protocol_event(&mut self, event: ProtocolEvent) -> Result<()> {
        match event {
            ProtocolEvent::ReceivedBlock {
                block_id,
                slot,
                storage,
            } => {
                massa_trace!(
                    "consensus.consensus_worker.process_protocol_event.received_block",
                    { "block_id": block_id }
                );
                self.block_db
                    .incoming_block(block_id, slot, self.previous_slot, storage)?;
                self.block_db_changed().await?;
            }
            ProtocolEvent::ReceivedBlockHeader { block_id, header } => {
                massa_trace!("consensus.consensus_worker.process_protocol_event.received_header", { "block_id": block_id, "header": header });
                self.block_db
                    .incoming_header(block_id, header, self.previous_slot)?;
                self.block_db_changed().await?;
            }
            ProtocolEvent::InvalidBlock { block_id, header } => {
                massa_trace!(
                    "consensus.consensus_worker.process_protocol_event.invalid_block",
                    { "block_id": block_id }
                );
                self.block_db.invalid_block(&block_id, header)?;
                // Say it to consensus
            }
        }
        Ok(())
    }

    /// prune statistics according to the stats span
    fn prune_stats(&mut self) -> Result<()> {
        let start_time =
            MassaTime::now(self.clock_compensation)?.saturating_sub(self.stats_history_timespan);
        self.final_block_stats.retain(|(t, _)| t >= &start_time);
        self.stale_block_stats.retain(|t| t >= &start_time);
        Ok(())
    }

    /// call me if the block database changed
    /// Processing of final blocks, pruning and producing endorsement.
    /// Please refactor me
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
    /// 11. Produce endorsements
    /// 12. add stale blocks to stats
    async fn block_db_changed(&mut self) -> Result<()> {
        massa_trace!("consensus.consensus_worker.block_db_changed", {});

        // Propagate new blocks
        for (block_id, storage) in self.block_db.get_blocks_to_propagate().into_iter() {
            massa_trace!("consensus.consensus_worker.block_db_changed.integrated", {
                "block_id": block_id
            });
            self.channels
                .protocol_command_sender
                .integrated_block(block_id, storage)
                .await?;
        }

        // Notify protocol of attack attempts.
        for hash in self.block_db.get_attack_attempts().into_iter() {
            self.channels
                .protocol_command_sender
                .notify_block_attack(hash)
                .await?;
            massa_trace!("consensus.consensus_worker.block_db_changed.attack", {
                "hash": hash
            });
        }

        // get new final blocks
        let new_final_block_ids = self.block_db.get_new_final_blocks();

        // get blockclique
        let blockclique_set = self.block_db.get_blockclique();

        // notify execution
        let final_blocks = new_final_block_ids
            .iter()
            .filter_map(|b_id| match self.block_db.get_active_block(b_id) {
                Some((a_b, storage)) if a_b.is_final => {
                    Some((a_b.slot, (a_b.block_id, storage.clone())))
                }
                _ => None,
            })
            .collect();
        let blockclique = blockclique_set
            .into_iter()
            .filter_map(|b_id| match self.block_db.get_active_block(&b_id) {
                Some((a_b, storage)) => Some((a_b.slot, (a_b.block_id, storage.clone()))),
                _ => None,
            })
            .collect();
        self.channels
            .execution_controller
            .update_blockclique_status(final_blocks, blockclique);

        // Process new final blocks
        let timestamp = MassaTime::now(self.clock_compensation)?;
        for b_id in new_final_block_ids.into_iter() {
            if let Some((a_block, _block_store)) = self.block_db.get_active_block(&b_id) {
                // add to stats
                self.final_block_stats
                    .push_back((timestamp, a_block.creator_address));
            }
        }

        // notify protocol of block wishlist
        let new_wishlist = self.block_db.get_block_wishlist()?;
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
                .send_wishlist_delta(new_blocks, remove_blocks)
                .await?;
            self.wishlist = new_wishlist;
        }

        // note new latest final periods
        let latest_final_periods: Vec<u64> = self
            .block_db
            .get_latest_final_blocks_periods()
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

        // add stale blocks to stats
        let new_stale_block_ids_creators_slots = self.block_db.get_new_stale_blocks();
        let timestamp = MassaTime::now(self.clock_compensation)?;
        for (_b_id, (_b_creator, _b_slot)) in new_stale_block_ids_creators_slots.into_iter() {
            self.stale_block_stats.push_back(timestamp);

            /*
            TODO add this again
            let creator_addr = Address::from_public_key(&b_creator);
            if self.staking_keys.contains_key(&creator_addr) {
                warn!("block {} that was produced by our address {} at slot {} became stale. This is probably due to a temporary desynchronization.", b_id, creator_addr, b_slot);
            }
            */
        }

        Ok(())
    }
}
