// Copyright (c) 2022 MASSA LABS <info@massa.net>
use massa_consensus_exports::{
    commands::ConsensusCommand,
    error::{ConsensusError, ConsensusResult as Result},
    settings::ConsensusWorkerChannels,
    ConsensusConfig,
};
use massa_graph::{BlockGraph, BlockGraphExport};
use massa_models::api::{LedgerInfo, RollsInfo};
use massa_models::prehash::{BuildMap, Map, Set};
use massa_models::timeslots::{get_block_slot_timestamp, get_latest_block_slot_at_timestamp};
use massa_models::{active_block::ActiveBlock, address::AddressState};
use massa_models::{stats::ConsensusStats, OperationId};
use massa_models::{Address, BlockId, Slot};
use massa_protocol_exports::{ProtocolEvent, ProtocolEventReceiver};
use massa_time::MassaTime;
use std::{cmp::max, collections::HashSet, collections::VecDeque};
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
    wishlist: Set<BlockId>,
    /// latest final periods
    latest_final_periods: Vec<u64>,
    /// clock compensation
    clock_compensation: i64,
    /// stats `(block -> tx_count, creator)`
    final_block_stats: VecDeque<(MassaTime, u64, Address)>,
    /// No idea what this is used for. My guess is one timestamp per stale block
    stale_block_stats: VecDeque<MassaTime>,
    /// the time span considered for stats
    stats_history_timespan: MassaTime,
    /// the time span considered for desynchronization detection
    #[allow(dead_code)]
    stats_desync_detection_timespan: MassaTime,
    /// time at which the node was launched (used for desynchronization detection)
    launch_time: MassaTime,
    /// endorsed slots cache
    endorsed_slots: HashSet<Slot>,
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
        let now = MassaTime::compensated_now(clock_compensation)?;
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
                0,
                genesis_addr,
            ))
        }

        // desync detection timespan
        let stats_desync_detection_timespan = cfg
            .t0
            .checked_mul(cfg.periods_per_cycle * (cfg.pos_lookback_cycles + 1))?;

        // notify execution module of current blockclique and final blocks
        // we need to do this because the bootstrap snapshots of the executor vs the consensus may not have been taken in sync
        // because the two modules run concurrently and out of sync

        let final_blocks = block_db.get_all_final_blocks();

        let blockclique = block_db
            .get_blockclique()
            .into_iter()
            .map(|block_id| {
                let a_block = block_db
                    .get_active_block(&block_id)
                    .expect("could not get active block for execution notification");
                (a_block.slot, (block_id, a_block.storage.clone()))
            })
            .collect();
        channels
            .execution_controller
            .update_blockclique_status(final_blocks, blockclique);

        Ok(ConsensusWorker {
            block_db,
            previous_slot,
            next_slot,
            wishlist: Set::<BlockId>::default(),
            latest_final_periods,
            clock_compensation,
            channels,
            final_block_stats,
            stale_block_stats: VecDeque::new(),
            stats_desync_detection_timespan,
            stats_history_timespan: max(stats_desync_detection_timespan, cfg.stats_timespan),
            cfg,
            launch_time: MassaTime::compensated_now(clock_compensation)?,
            endorsed_slots: HashSet::new(),
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
                        if MassaTime::compensated_now(self.clock_compensation)? > end {
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
        let now = MassaTime::compensated_now(self.clock_compensation)?;
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
            // return full block and status with specified hash
            ConsensusCommand::GetBlockStatus {
                block_id,
                response_tx,
            } => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_block_status",
                    {}
                );
                if response_tx
                    .send(self.block_db.get_export_block_status(&block_id)?)
                    .is_err()
                {
                    warn!("consensus: could not send GetBlock Status answer");
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
                if response_tx.send(resp).is_err() {
                    warn!("consensus: could not send GetBootstrapState answer");
                }
                Ok(())
            }
            ConsensusCommand::GetAddressesInfo {
                addresses,
                response_tx,
            } => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_addresses_info",
                    { "addresses": addresses }
                );
                if response_tx
                    .send(self.get_addresses_info(&addresses)?)
                    .is_err()
                {
                    warn!("consensus: could not send GetAddressesInfo response");
                }
                Ok(())
            }
            ConsensusCommand::GetRecentOperations {
                address,
                response_tx,
            } => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_operations_involving_address",
                    { "address": address }
                );

                if response_tx
                    .send(self.block_db.get_operations_involving_address(&address)?)
                    .is_err()
                {
                    warn!("consensus: could not send GetRecentOPerations response");
                }
                Ok(())
            }
            ConsensusCommand::GetOperations {
                operation_ids,
                response_tx,
            } => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_operations",
                    { "operation_ids": operation_ids }
                );
                if response_tx
                    .send(self.block_db.get_operations(operation_ids)?)
                    .is_err()
                {
                    warn!("consensus: could not send get operations response");
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
            ConsensusCommand::GetBlockIdsByCreator {
                address,
                response_tx,
            } => {
                if response_tx
                    .send(self.block_db.get_block_ids_by_creator(&address))
                    .is_err()
                {
                    warn!("consensus: could not send get block ids by creator response");
                }
                Ok(())
            }
            ConsensusCommand::GetEndorsementsByAddress {
                address,
                response_tx,
            } => response_tx
                .send(self.block_db.get_endorsement_by_address(address)?)
                .map_err(|err| {
                    ConsensusError::SendChannelError(format!(
                        "could not send get endorsement by address response: {:?}",
                        err
                    ))
                }),
            ConsensusCommand::GetEndorsementsById {
                endorsements,
                response_tx,
            } => response_tx
                .send(self.block_db.get_endorsement_by_id(endorsements)?)
                .map_err(|err| {
                    ConsensusError::SendChannelError(format!(
                        "could not send get endorsement by id response: {:?}",
                        err
                    ))
                }),
        }
    }

    /// retrieve stats
    /// Used in response to a API request
    fn get_stats(&mut self) -> Result<ConsensusStats> {
        let timespan_end = max(
            self.launch_time,
            MassaTime::compensated_now(self.clock_compensation)?,
        );
        let timespan_start = max(
            timespan_end.saturating_sub(self.cfg.stats_timespan),
            self.launch_time,
        );
        let final_block_count = self
            .final_block_stats
            .iter()
            .filter(|(t, _, _)| *t >= timespan_start && *t < timespan_end)
            .count() as u64;
        let final_operation_count = self
            .final_block_stats
            .iter()
            .filter(|(t, _, _)| *t >= timespan_start && *t < timespan_end)
            .fold(0u64, |acc, (_, tx_n, _)| acc + tx_n);
        let stale_block_count = self
            .stale_block_stats
            .iter()
            .filter(|t| **t >= timespan_start && **t < timespan_end)
            .count() as u64;
        let clique_count = self.block_db.get_clique_count() as u64;
        Ok(ConsensusStats {
            final_block_count,
            final_operation_count,
            stale_block_count,
            clique_count,
            start_timespan: timespan_start,
            end_timespan: timespan_end,
            staker_count: 0, // TODO get staker counts back
        })
    }

    /// all you wanna know about an address
    /// Used in response to a API request
    fn get_addresses_info(&self, addresses: &Set<Address>) -> Result<Map<Address, AddressState>> {
        let thread_count = self.cfg.thread_count;
        let mut addresses_by_thread = vec![Set::<Address>::default(); thread_count as usize];
        for addr in addresses.iter() {
            addresses_by_thread[addr.get_thread(thread_count) as usize].insert(*addr);
        }
        let mut states = Map::default();
        for thread in 0..thread_count {
            if addresses_by_thread[thread as usize].is_empty() {
                continue;
            }

            for addr in addresses_by_thread[thread as usize].iter() {
                states.insert(
                    *addr,
                    AddressState {
                        rolls: RollsInfo {
                            final_rolls: Default::default(),     // TODO update with new PoS,
                            active_rolls: Default::default(),    // TODO update with new PoS
                            candidate_rolls: Default::default(), // TODO get from execution module
                        },
                        ledger_info: LedgerInfo {
                            locked_balance: Default::default(), // TODO get from exec module
                            candidate_ledger_info: Default::default(), // TODO get from execution module
                            final_ledger_info: Default::default(),     // TODO get from exec module
                        },

                        production_stats: Default::default(), /* TODO repair this  let prod_stats = self.pos.get_stakers_production_stats(addresses);self.pos.get_stakers_production_stats(addresses);
                                                              prod_stats
                                                                  .iter()
                                                                  .map(|cycle_stats| AddressCycleProductionStats {
                                                                      cycle: cycle_stats.cycle,
                                                                      is_final: cycle_stats.is_final,
                                                                      ok_count: cycle_stats.ok_nok_counts.get(addr).unwrap_or(&(0, 0)).0,
                                                                      nok_count: cycle_stats.ok_nok_counts.get(addr).unwrap_or(&(0, 0)).1,
                                                                  })
                                                                  .collect(),*/
                    },
                );
            }
        }
        Ok(states)
    }

    /// Manages received protocol events.
    ///
    /// # Arguments
    /// * `event`: event type to process.
    async fn process_protocol_event(&mut self, event: ProtocolEvent) -> Result<()> {
        match event {
            ProtocolEvent::ReceivedBlock {
                block,
                slot,
                operation_set,
                endorsement_ids,
            } => {
                massa_trace!(
                    "consensus.consensus_worker.process_protocol_event.received_block",
                    { "block_id": block.id }
                );
                let block_id = block.id;

                // Store block in shared storage.
                self.block_db.storage.store_block(block);

                self.block_db.incoming_block(
                    block_id,
                    slot,
                    operation_set,
                    endorsement_ids,
                    self.previous_slot,
                )?;
                self.block_db_changed().await?;
            }
            ProtocolEvent::ReceivedBlockHeader { block_id, header } => {
                massa_trace!("consensus.consensus_worker.process_protocol_event.received_header", { "block_id": block_id, "header": header });
                self.block_db
                    .incoming_header(block_id, header, self.previous_slot)?;
                self.block_db_changed().await?;
            }
            ProtocolEvent::GetBlocks(list) => {
                massa_trace!(
                    "consensus.consensus_worker.process_protocol_event.get_blocks",
                    { "list": list }
                );
                let mut results = Map::default();
                for block_hash in list {
                    if let Some(a_block) = self.block_db.get_active_block(&block_hash) {
                        massa_trace!("consensus.consensus_worker.process_protocol_event.get_block.consensus_found", { "hash": block_hash});
                        results.insert(
                            block_hash,
                            Some((
                                Some(a_block.operation_set.keys().copied().collect()),
                                Some(a_block.endorsement_ids.keys().copied().collect()),
                            )),
                        );
                    } else {
                        // not found in consensus
                        massa_trace!("consensus.consensus_worker.process_protocol_event.get_block.consensus_not_found", { "hash": block_hash});
                        results.insert(block_hash, None);
                    }
                }
                self.channels
                    .protocol_command_sender
                    .send_get_blocks_results(results)
                    .await?;
            }
        }
        Ok(())
    }

    /// prune statistics according to the stats span
    fn prune_stats(&mut self) -> Result<()> {
        let start_time = MassaTime::compensated_now(self.clock_compensation)?
            .saturating_sub(self.stats_history_timespan);
        self.final_block_stats.retain(|(t, _, _)| t >= &start_time);
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
        for (block_id, (op_ids, endo_ids)) in self.block_db.get_blocks_to_propagate().into_iter() {
            massa_trace!("consensus.consensus_worker.block_db_changed.integrated", {
                "block_id": block_id
            });
            self.channels
                .protocol_command_sender
                .integrated_block(block_id, op_ids, endo_ids)
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

        let local_storage = self.block_db.storage.clone_without_refs();
        // notify execution
        let final_blocks = new_final_block_ids
            .clone()
            .into_iter()
            .filter_map(|b_id| match self.block_db.get_active_block(&b_id) {
                Some(a_b) if a_b.is_final => Some(self.get_block_slot(a_b)),
                _ => None,
            })
            .collect();
        let blockclique = blockclique_set
            .clone()
            .into_iter()
            .filter_map(|block_id| {
                self.block_db
                    .get_active_block(&block_id)
                    .map(|a_block| self.get_block_slot(a_block))
            })
            .collect();
        self.channels
            .execution_controller
            .update_blockclique_status(final_blocks, blockclique);

        // Process new final blocks
        let mut new_final_ops: Map<OperationId, (u64, u8)> = Map::default();
        let mut new_final_blocks =
            Map::with_capacity_and_hasher(new_final_block_ids.len(), BuildMap::default());
        let timestamp = MassaTime::compensated_now(self.clock_compensation)?;
        for b_id in new_final_block_ids.into_iter() {
            if let Some(a_block) = self.block_db.get_active_block(&b_id) {
                // List new final ops
                new_final_ops.extend(
                    a_block
                        .operation_set
                        .iter()
                        .map(|(id, (_, exp))| (*id, (*exp, a_block.slot.thread))),
                );
                // List final block
                new_final_blocks.insert(b_id, a_block);
                // add to stats
                self.final_block_stats.push_back((
                    timestamp,
                    a_block.operation_set.len() as u64,
                    a_block.creator_address,
                ));
            }
        }

        // notify protocol of block wishlist
        let new_wishlist = self.block_db.get_block_wishlist()?;
        let new_blocks = &new_wishlist - &self.wishlist;
        let remove_blocks = &self.wishlist - &new_wishlist;
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
            // discard endorsed slots cache
            self.endorsed_slots
                .retain(|s| s.period >= latest_final_periods[s.thread as usize]);

            // update final periods
            self.latest_final_periods = latest_final_periods;
        }

        // add stale blocks to stats
        let new_stale_block_ids_creators_slots = self.block_db.get_new_stale_blocks();
        let timestamp = MassaTime::compensated_now(self.clock_compensation)?;
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
