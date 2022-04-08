// Copyright (c) 2022 MASSA LABS <info@massa.net>
use massa_consensus_exports::{
    commands::ConsensusCommand,
    error::{ConsensusError, ConsensusResult as Result},
    events::ConsensusEvent,
    settings::ConsensusWorkerChannels,
    ConsensusConfig,
};
use massa_graph::{BlockGraph, BlockGraphExport};
use massa_hash::Hash;
use massa_models::prehash::{BuildMap, Map, Set};
use massa_models::timeslots::{get_block_slot_timestamp, get_latest_block_slot_at_timestamp};
use massa_models::{address::AddressCycleProductionStats, stats::ConsensusStats, OperationId};
use massa_models::{address::AddressState, signed::Signed};
use massa_models::{
    api::{LedgerInfo, RollsInfo},
    SignedEndorsement,
};
use massa_models::{ledger_models::LedgerData, SignedOperation};
use massa_models::{
    Address, Block, BlockHeader, BlockId, Endorsement, EndorsementId, SerializeCompact, Slot,
};
use massa_proof_of_stake_exports::{error::ProofOfStakeError, ExportProofOfStake, ProofOfStake};
use massa_protocol_exports::{ProtocolEvent, ProtocolEventReceiver};
use massa_signature::{derive_public_key, PrivateKey, PublicKey};
use massa_time::MassaTime;
use std::{cmp::max, collections::HashSet, collections::VecDeque};
use tokio::{
    sync::mpsc::error::SendTimeoutError,
    time::{sleep, sleep_until, Sleep},
};
use tracing::{debug, info, warn};

/// Manages consensus.
pub struct ConsensusWorker {
    /// Consensus Configuration
    cfg: ConsensusConfig,
    /// Genesis blocks we recreated with that public key.
    genesis_public_key: PublicKey,
    /// Associated channels, sender and receivers
    channels: ConsensusWorkerChannels,
    /// Database containing all information about blocks, the blockgraph and cliques.
    block_db: BlockGraph,
    /// Proof of stake module.
    pos: ProofOfStake,
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
    /// staking keys
    staking_keys: Map<Address, (PublicKey, PrivateKey)>,
    /// stats (block -> tx_count, creator)
    final_block_stats: VecDeque<(MassaTime, u64, Address)>,
    /// No idea what this is used for. My guess is one timestamp per stale block
    stale_block_stats: VecDeque<MassaTime>,
    /// the time span considered for stats
    stats_history_timespan: MassaTime,
    /// the time span considered for desync detection
    stats_desync_detection_timespan: MassaTime,
    /// time at which the node was launched (used for desync detection)
    launch_time: MassaTime,
    // endorsed slots cache
    endorsed_slots: HashSet<Slot>,
}

impl ConsensusWorker {
    /// Creates a new consensus controller.
    /// Initiates the random selector.
    ///
    /// # Arguments
    /// * cfg: consensus configuration.
    /// * protocol_command_sender: associated protocol controller
    /// * block_db: Database containing all information about blocks, the blockgraph and cliques.
    /// * controller_command_rx: Channel receiving consensus commands.
    /// * controller_event_tx: Channel sending out consensus events.
    /// * controller_manager_rx: Channel receiving consensus management commands.
    pub(crate) async fn new(
        cfg: ConsensusConfig,
        channels: ConsensusWorkerChannels,
        block_db: BlockGraph,
        pos: ProofOfStake,
        clock_compensation: i64,
        staking_keys: Map<Address, (PublicKey, PrivateKey)>,
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
        for addr in staking_keys.keys() {
            info!("Staking enabled for address: {}", addr);
        }
        massa_trace!("consensus.consensus_worker.new", {});

        // add genesis blocks to stats
        let genesis_public_key = derive_public_key(&cfg.genesis_key);
        let genesis_addr = Address::from_public_key(&genesis_public_key);
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
        channels.execution_controller.update_blockclique_status(
            block_db.get_all_final_blocks(),
            block_db
                .get_blockclique()
                .into_iter()
                .filter_map(|block_id| {
                    block_db
                        .get_active_block(&block_id)
                        .map(|a_block| (a_block.slot, block_id))
                })
                .collect(),
        );

        Ok(ConsensusWorker {
            genesis_public_key,
            block_db,
            pos,
            previous_slot,
            next_slot,
            wishlist: Set::<BlockId>::default(),
            latest_final_periods,
            clock_compensation,
            channels,
            staking_keys,
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
        if let Some(previous_slot) = self.previous_slot {
            self.channels
                .pool_command_sender
                .update_current_slot(previous_slot)
                .await?;
        }
        self.channels
            .pool_command_sender
            .update_latest_final_periods(self.latest_final_periods.clone())
            .await?;

        // set slot timer
        let next_slot_timer = sleep_until(
            get_block_slot_timestamp(
                self.cfg.thread_count,
                self.cfg.t0,
                self.cfg.genesis_timestamp,
                self.next_slot,
            )?
            .estimate_instant(self.clock_compensation)?,
        );

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
    /// it checks for cycle incrementation
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
            next_slot_timer.set(sleep_until(
                get_block_slot_timestamp(
                    self.cfg.thread_count,
                    self.cfg.t0,
                    self.cfg.genesis_timestamp,
                    self.next_slot,
                )?
                .estimate_instant(self.clock_compensation)?,
            ));
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

        // signal tick to pool
        self.channels
            .pool_command_sender
            .update_current_slot(observed_slot)
            .await?;

        // create blocks
        if !self.cfg.disable_block_creation && observed_slot.period > 0 {
            let mut cur_slot = self.next_slot;
            while cur_slot <= observed_slot {
                let block_draw = match self.pos.draw_block_producer(cur_slot) {
                    Ok(b_draw) => Some(b_draw),
                    Err(ProofOfStakeError::PosCycleUnavailable(_)) => {
                        massa_trace!(
                            "consensus.consensus_worker.slot_tick.block_creatorunavailable",
                            {}
                        );
                        warn!("desynchronization detected because the lookback cycle is not final at the current time");
                        let _ = self.send_consensus_event(ConsensusEvent::NeedSync).await;
                        None
                    }
                    Err(err) => return Err(err.into()),
                };
                if let Some(addr) = block_draw {
                    if let Some((pub_k, priv_k)) = self.staking_keys.get(&addr).cloned() {
                        massa_trace!("consensus.consensus_worker.slot_tick.block_creator_addr", { "addr": addr, "pubkey": pub_k, "unlocked": true });
                        self.create_block(cur_slot, &addr, &pub_k, &priv_k).await?;
                        if let Some(next_addr_slot) =
                            self.pos.get_next_selected_slot(self.next_slot, addr)
                        {
                            info!(
                            "Next block creation slot for address {}: cycle {}, period {}, thread {}, at time {}",
                            addr,
                            next_addr_slot.get_cycle(self.cfg.periods_per_cycle),
                            next_addr_slot.period,
                            next_addr_slot.thread,
                            match get_block_slot_timestamp(
                                self.cfg.thread_count,
                                self.cfg.t0,
                                self.cfg.genesis_timestamp,
                                next_addr_slot
                            ) {
                                Ok(time) => time.to_utc_string(),
                                Err(err) =>
                                    format!("(internal error during get_block_slot_timestamp: {})", err),
                            }
                        );
                        }
                    } else {
                        massa_trace!("consensus.consensus_worker.slot_tick.block_creator_addr", { "addr": addr, "unlocked": false });
                    }
                }
                cur_slot = cur_slot.get_next_slot(self.cfg.thread_count)?;
            }
        }

        self.previous_slot = Some(observed_slot);
        self.next_slot = observed_slot.get_next_slot(self.cfg.thread_count)?;

        // signal tick to block graph
        self.block_db
            .slot_tick(&mut self.pos, Some(observed_slot))?;

        // take care of block db changes
        self.block_db_changed().await?;

        // reset timer for next slot
        next_slot_timer.set(sleep_until(
            get_block_slot_timestamp(
                self.cfg.thread_count,
                self.cfg.t0,
                self.cfg.genesis_timestamp,
                self.next_slot,
            )?
            .estimate_instant(self.clock_compensation)?,
        ));

        // prune stats
        self.prune_stats()?;

        Ok(())
    }

    /// creates a block with given address
    /// first an empty block is created then it's filled with operations
    /// the operations are retrieved from the pool
    /// the block is added to the graph as it it was received from the outside
    /// so it will on go the same checks
    async fn create_block(
        &mut self,
        cur_slot: Slot,
        creator_addr: &Address,
        creator_public_key: &PublicKey,
        creator_private_key: &PrivateKey,
    ) -> Result<()> {
        // get parents
        let parents = self.block_db.get_best_parents();
        let (thread_parent, thread_parent_period) = parents[cur_slot.thread as usize];

        // get endorsements
        // it is assumed that only valid endorsements in that context are selected by pool
        let (endorsement_ids, endorsements) = if thread_parent_period > 0 {
            let thread_parent_slot = Slot::new(thread_parent_period, cur_slot.thread);
            let endorsement_draws = self.pos.draw_endorsement_producers(thread_parent_slot)?;
            self.channels
                .pool_command_sender
                .get_endorsements(thread_parent_slot, thread_parent, endorsement_draws)
                .await?
                .into_iter()
                .map(|(id, e)| ((id, e.content.index), e))
                .unzip()
        } else {
            (Map::default(), Vec::new())
        };

        massa_trace!("consensus.create_block.get_endorsements.result", {
            "endorsements": endorsements
        });

        // create empty block
        let (_block_id, header) = Signed::new_signed(
            BlockHeader {
                creator: *creator_public_key,
                slot: cur_slot,
                parents: parents.iter().map(|(b, _p)| *b).collect(),
                operation_merkle_root: Hash::compute_from(&Vec::new()[..]),
                endorsements: endorsements.clone(),
            },
            creator_private_key,
        )?;
        let block = Block {
            header,
            operations: Vec::new(),
        };

        let serialized_block = block.to_bytes_compact()?;

        // initialize remaining block space and remaining operation count
        let mut remaining_block_space = (self.cfg.max_block_size as u64)
            .checked_sub(serialized_block.len() as u64)
            .ok_or_else(|| {
                ConsensusError::BlockCreationError(format!(
                    "consensus config max_block_size ({}) is smaller than an empty block ({})",
                    self.cfg.max_block_size,
                    serialized_block.len()
                ))
            })?;
        let mut remaining_operation_count = self.cfg.max_operations_per_block as usize;

        // exclude operations that were used in block ancestry
        let mut exclude_operations = Set::<OperationId>::default();
        let mut ancestor_id = block.header.content.parents[cur_slot.thread as usize];
        let stop_period = cur_slot
            .period
            .saturating_sub(self.cfg.operation_validity_periods);
        loop {
            let ancestor = self
                .block_db
                .get_active_block(&ancestor_id)
                .ok_or_else(|| {
                    ConsensusError::ContainerInconsistency(format!(
                        "missing ancestor to check operation reuse for block creation: {}",
                        ancestor_id
                    ))
                })?;
            if ancestor.slot.period < stop_period {
                break;
            }
            exclude_operations.extend(ancestor.operation_set.keys());
            if ancestor.parents.is_empty() {
                break;
            }
            ancestor_id = ancestor.parents[cur_slot.thread as usize].0;
        }

        // init block state accumulator
        let mut state_accu = self
            .block_db
            .block_state_accumulator_init(&block.header, &mut self.pos)?;

        // gather operations
        let mut total_hash: Vec<u8> = Vec::new();
        let mut operations: Vec<SignedOperation> = Vec::new();
        let mut operation_set: Map<OperationId, (usize, u64)> = Map::default(); // (index, validity end period)
        let mut finished = remaining_block_space == 0
            || remaining_operation_count == 0
            || self.cfg.max_operations_fill_attempts == 0;
        let mut attempts = 0;
        let mut total_gas = 0u64;
        while !finished {
            // get a batch of operations
            let operation_batch = self
                .channels
                .pool_command_sender
                .send_get_operations_announcement(
                    cur_slot,
                    exclude_operations.clone(),
                    self.cfg.operation_batch_size,
                    remaining_block_space,
                )
                .await?;

            attempts += 1;

            // Finish once we receive a batch that isn't full,
            // or if the maximum number of attempts has been reached.
            finished = operation_batch.len() < self.cfg.operation_batch_size
                || self.cfg.max_operations_fill_attempts == attempts;

            for (op_id, op, op_size) in operation_batch.into_iter() {
                // exclude operation from future batches
                exclude_operations.insert(op_id);

                // check that the operation fits in size
                if op_size > remaining_block_space {
                    continue;
                }

                // check that we have block gas left
                let op_gas = op.content.get_gas_usage();
                if total_gas.saturating_add(op_gas) > self.cfg.max_gas_per_block {
                    // no more gas left: do not include
                    continue;
                }

                // try to apply operation to block state
                // on failure, the block state is not modified
                if self
                    .block_db
                    .block_state_try_apply_op(&mut state_accu, &block.header, &op, &mut self.pos)
                    .is_err()
                {
                    continue;
                };

                // add operation
                operation_set.insert(op_id, (operation_set.len(), op.content.expire_period));
                operations.push(op);
                remaining_block_space -= op_size;
                remaining_operation_count -= 1;
                total_gas += op_gas;
                total_hash.extend(op_id.to_bytes());

                // check if the block still has some space
                if remaining_block_space == 0 || remaining_operation_count == 0 {
                    finished = true;
                    break;
                }
            }
        }

        // compile resulting block
        let (block_id, header) = Signed::new_signed(
            BlockHeader {
                creator: *creator_public_key,
                slot: cur_slot,
                parents: parents.iter().map(|(b, _p)| *b).collect(),
                operation_merkle_root: Hash::compute_from(&total_hash),
                endorsements,
            },
            creator_private_key,
        )?;
        let block = Block { header, operations };
        let slot = block.header.content.slot;
        massa_trace!("create block", { "block": block });

        let serialized_block = block.to_bytes_compact()?;

        // Add to shared storage
        self.block_db
            .storage
            .store_block(block_id, block, serialized_block);

        info!(
            "Staked block {} with address {}, at cycle {}, period {}, thread {}",
            block_id,
            creator_addr,
            cur_slot.get_cycle(self.cfg.periods_per_cycle),
            cur_slot.period,
            cur_slot.thread
        );

        // add block to db
        self.block_db.incoming_block(
            block_id,
            slot,
            operation_set,
            endorsement_ids,
            &mut self.pos,
            Some(cur_slot),
        )?;

        Ok(())
    }

    /// Channel management stuff
    /// todo delete
    /// or at least introduce some genericity
    async fn send_consensus_event(&self, event: ConsensusEvent) -> Result<()> {
        let result = self
            .channels
            .controller_event_tx
            .send_timeout(event, self.cfg.max_send_wait.to_duration())
            .await;
        match result {
            Ok(()) => return Ok(()),
            Err(SendTimeoutError::Closed(event)) => {
                debug!(
                    "failed to send ConsensusEvent due to channel closure: {:?}",
                    event
                );
            }
            Err(SendTimeoutError::Timeout(event)) => {
                debug!("failed to send ConsensusEvent due to timeout: {:?}", event);
            }
        }
        Err(ConsensusError::ChannelError("failed to send event".into()))
    }

    /// Manages given consensus command.
    /// They can come from the api or the bootstrap server
    /// Please refactor me
    ///
    /// # Argument
    /// * cmd: consensus command to process
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
            ConsensusCommand::GetSelectionDraws {
                start,
                end,
                response_tx,
            } => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_selection_draws",
                    {}
                );
                let mut res = Vec::new();
                let mut cur_slot = start;
                let result = loop {
                    if cur_slot >= end {
                        break Ok(res);
                    }

                    res.push((
                        cur_slot,
                        if cur_slot.period == 0 {
                            (
                                Address::from_public_key(&self.genesis_public_key),
                                Vec::new(),
                            )
                        } else {
                            (
                                match self.pos.draw_block_producer(cur_slot) {
                                    Err(err) => break Err(ConsensusError::from(err)),
                                    Ok(block_addr) => block_addr,
                                },
                                match self.pos.draw_endorsement_producers(cur_slot) {
                                    Err(err) => break Err(err.into()),
                                    Ok(endo_addrs) => endo_addrs,
                                },
                            )
                        },
                    ));
                    cur_slot = match cur_slot.get_next_slot(self.cfg.thread_count) {
                        Ok(next_slot) => next_slot,
                        Err(_) => {
                            break Err(ConsensusError::SlotOverflowError);
                        }
                    }
                };
                if response_tx.send(result).is_err() {
                    warn!("consensus: could not send GetSelectionDraws response");
                }
                Ok(())
            }
            ConsensusCommand::GetBootstrapState(response_tx) => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_bootstrap_state",
                    {}
                );
                let resp = (
                    ExportProofOfStake::from(&self.pos),
                    self.block_db.export_bootstrap_graph()?,
                );
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
                    .send(self.block_db.get_operations(&operation_ids)?)
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
            ConsensusCommand::GetActiveStakers(response_tx) => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_active_stakers",
                    {}
                );
                let res = self.get_active_stakers()?;
                if response_tx.send(res).is_err() {
                    warn!("consensus: could not send get_active_stakers response");
                }
                Ok(())
            }
            ConsensusCommand::RegisterStakingPrivateKeys(keys) => {
                for key in keys.into_iter() {
                    let public = derive_public_key(&key);
                    let address = Address::from_public_key(&public);
                    info!("Staking with address {}", address);
                    self.staking_keys.insert(address, (public, key));
                }
                self.pos
                    .set_watched_addresses(self.staking_keys.keys().copied().collect());
                self.dump_staking_keys().await;
                Ok(())
            }
            ConsensusCommand::RemoveStakingAddresses(addresses) => {
                for address in addresses.into_iter() {
                    self.staking_keys.remove(&address);
                }
                self.pos
                    .set_watched_addresses(self.staking_keys.keys().copied().collect());
                self.dump_staking_keys().await;
                Ok(())
            }
            ConsensusCommand::GetStakingAddressses(response_tx) => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_staking_addresses",
                    {}
                );
                if response_tx
                    .send(self.staking_keys.keys().cloned().collect())
                    .is_err()
                {
                    warn!("consensus: could not send get_staking addresses response");
                }
                Ok(())
            }
            ConsensusCommand::GetStakersProductionStats { addrs, response_tx } => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_stakers_production_stats",
                    {}
                );
                if response_tx
                    .send(self.pos.get_stakers_production_stats(&addrs))
                    .is_err()
                {
                    warn!("consensus: could not send get_staking addresses response");
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

    /// Save the staking keys to a file
    async fn dump_staking_keys(&self) {
        let keys = self
            .staking_keys
            .iter()
            .map(|(_, (_, key))| *key)
            .collect::<Vec<_>>();
        let json = match serde_json::to_string_pretty(&keys) {
            Ok(json) => json,
            Err(e) => {
                warn!("Error while serializing staking keys {}", e);
                return;
            }
        };

        if let Err(e) = tokio::fs::write(self.cfg.staking_keys_path.clone(), json).await {
            warn!("Error while dumping staking keys {}", e);
        }
    }

    /// retrieve stats
    /// Used in response to a api request
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
        let target_cycle = self.next_slot.get_cycle(self.cfg.periods_per_cycle);
        Ok(ConsensusStats {
            final_block_count,
            final_operation_count,
            stale_block_count,
            clique_count,
            start_timespan: timespan_start,
            end_timespan: timespan_end,
            staker_count: self.pos.get_stakers_count(target_cycle)?,
        })
    }

    /// retrieve all current cycle active stakers
    /// Used in response to a api request
    fn get_active_stakers(&self) -> Result<Map<Address, u64>, ConsensusError> {
        let cur_cycle = self.next_slot.get_cycle(self.cfg.periods_per_cycle);
        let mut res: Map<Address, u64> = Map::default();
        for thread in 0..self.cfg.thread_count {
            match self.pos.get_lookback_roll_count(cur_cycle, thread) {
                Ok(rolls) => {
                    res.extend(&rolls.0);
                }
                Err(err) => return Err(err.into()),
            }
        }
        Ok(res)
    }

    /// all you wanna know about an address
    /// Used in response to a api request
    fn get_addresses_info(&self, addresses: &Set<Address>) -> Result<Map<Address, AddressState>> {
        let thread_count = self.cfg.thread_count;
        let mut addresses_by_thread = vec![Set::<Address>::default(); thread_count as usize];
        for addr in addresses.iter() {
            addresses_by_thread[addr.get_thread(thread_count) as usize].insert(*addr);
        }
        let mut states = Map::default();
        let cur_cycle = self.next_slot.get_cycle(self.cfg.periods_per_cycle);
        let ledger_data = self.block_db.get_ledger_data_export(addresses)?;
        let prod_stats = self.pos.get_stakers_production_stats(addresses);
        for thread in 0..thread_count {
            if addresses_by_thread[thread as usize].is_empty() {
                continue;
            }

            let lookback_data = match self.pos.get_lookback_roll_count(cur_cycle, thread) {
                Ok(rolls) => Some(rolls),
                Err(ProofOfStakeError::PosCycleUnavailable(_)) => None,
                Err(e) => return Err(e.into()),
            };
            let final_data = self
                .pos
                .get_final_roll_data(self.pos.get_last_final_block_cycle(thread), thread)
                .ok_or_else(|| {
                    ProofOfStakeError::PosCycleUnavailable("final cycle unavailable".to_string())
                })?;
            let candidate_data = self.block_db.get_roll_data_at_parent(
                self.block_db.get_best_parents()[thread as usize].0,
                Some(&addresses_by_thread[thread as usize]),
                &self.pos,
            )?;
            let locked_rolls = self.pos.get_locked_roll_count(
                cur_cycle,
                thread,
                &addresses_by_thread[thread as usize],
            );

            for addr in addresses_by_thread[thread as usize].iter() {
                states.insert(
                    *addr,
                    AddressState {
                        rolls: RollsInfo {
                            final_rolls: *final_data.roll_count.0.get(addr).unwrap_or(&0),
                            active_rolls: lookback_data
                                .map(|data| *data.0.get(addr).unwrap_or(&0))
                                .unwrap_or(0),
                            candidate_rolls: *candidate_data.0 .0.get(addr).unwrap_or(&0),
                        },
                        ledger_info: LedgerInfo {
                            locked_balance: self
                                .cfg
                                .roll_price
                                .checked_mul_u64(*locked_rolls.get(addr).unwrap_or(&0))
                                .ok_or(ProofOfStakeError::RollOverflowError)?,
                            candidate_ledger_info: ledger_data
                                .candidate_data
                                .0
                                .get(addr)
                                .map_or_else(LedgerData::default, |v| *v),
                            final_ledger_info: ledger_data
                                .final_data
                                .0
                                .get(addr)
                                .map_or_else(LedgerData::default, |v| *v),
                        },

                        production_stats: prod_stats
                            .iter()
                            .map(|cycle_stats| AddressCycleProductionStats {
                                cycle: cycle_stats.cycle,
                                is_final: cycle_stats.is_final,
                                ok_count: cycle_stats.ok_nok_counts.get(addr).unwrap_or(&(0, 0)).0,
                                nok_count: cycle_stats.ok_nok_counts.get(addr).unwrap_or(&(0, 0)).1,
                            })
                            .collect(),
                    },
                );
            }
        }
        Ok(states)
    }

    /// Manages received protocol events.
    ///
    /// # Arguments
    /// * event: event type to process.
    async fn process_protocol_event(&mut self, event: ProtocolEvent) -> Result<()> {
        match event {
            ProtocolEvent::ReceivedBlock {
                block_id,
                slot,
                operation_set,
                endorsement_ids,
            } => {
                massa_trace!(
                    "consensus.consensus_worker.process_protocol_event.received_block",
                    { "block_id": block_id }
                );
                self.block_db.incoming_block(
                    block_id,
                    slot,
                    operation_set,
                    endorsement_ids,
                    &mut self.pos,
                    self.previous_slot,
                )?;
                self.block_db_changed().await?;
            }
            ProtocolEvent::ReceivedBlockHeader { block_id, header } => {
                massa_trace!("consensus.consensus_worker.process_protocol_event.received_header", { "block_id": block_id, "header": header });
                self.block_db.incoming_header(
                    block_id,
                    header,
                    &mut self.pos,
                    self.previous_slot,
                )?;
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
                        massa_trace!("consensus.consensus_worker.process_protocol_event.get_block.consensu_not_found", { "hash": block_hash});
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
    /// 9. notify protocol of block wishlist
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

        // notify execution
        self.channels
            .execution_controller
            .update_blockclique_status(
                new_final_block_ids
                    .clone()
                    .into_iter()
                    .filter_map(|b_id| {
                        if let Some(a_b) = self.block_db.get_active_block(&b_id) {
                            if a_b.is_final {
                                return Some((a_b.slot, b_id));
                            }
                        }
                        None
                    })
                    .collect(),
                blockclique_set
                    .clone()
                    .into_iter()
                    .filter_map(|block_id| {
                        self.block_db
                            .get_active_block(&block_id)
                            .map(|a_block| (a_block.slot, block_id))
                    })
                    .collect(),
            );

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
        // Notify pool of new final ops
        if !new_final_ops.is_empty() {
            self.channels
                .pool_command_sender
                .final_operations(new_final_ops)
                .await?;
        }
        // Notify PoS of final blocks
        self.pos.note_final_blocks(new_final_blocks)?;

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

            // send latest final periods to pool
            self.channels
                .pool_command_sender
                .update_latest_final_periods(self.latest_final_periods.clone())
                .await?;
        }

        // Produce endorsements
        if !self.cfg.disable_block_creation {
            // iterate on all blockclique blocks
            for block_id in blockclique_set.into_iter() {
                let block_slot = match self.block_db.get_active_block(&block_id) {
                    Some(b) => b.slot,
                    None => continue,
                };
                if self.endorsed_slots.contains(&block_slot) {
                    // skip already endorsed (to prevent double stake pernalty)
                    continue;
                }
                // check that the block to endorse is at most one period before the last slot
                if let Some(prev_slot) = self.previous_slot {
                    if prev_slot.period.saturating_sub(block_slot.period) > 1 {
                        continue;
                    }
                } else {
                    continue;
                }
                // check endorsement draws
                let endorsement_draws = match self.pos.draw_endorsement_producers(block_slot) {
                    Ok(e_draws) => e_draws,
                    Err(err) => {
                        warn!(
                            "could not draw endorsements at slot {}: {}",
                            block_slot, err
                        );
                        Vec::new()
                    }
                };

                // actually create endorsements
                let mut endorsements = Map::default();
                for (endorsement_index, addr) in endorsement_draws.into_iter().enumerate() {
                    if let Some((pub_k, priv_k)) = self.staking_keys.get(&addr) {
                        massa_trace!("consensus.consensus_worker.slot_tick.endorsement_creator_addr",
                            { "index": endorsement_index, "addr": addr, "pubkey": pub_k, "unlocked": true });
                        let (endorsement_id, endorsement) = create_endorsement(
                            block_slot,
                            *pub_k,
                            priv_k,
                            endorsement_index as u32,
                            block_id,
                        )?;
                        endorsements.insert(endorsement_id, endorsement);
                        self.endorsed_slots.insert(block_slot);
                    } else {
                        massa_trace!("consensus.consensus_worker.slot_tick.endorsement_creator_addr",
                            { "index": endorsement_index, "addr": addr, "unlocked": false });
                    }
                }
                // send endorsement batch to pool
                if !endorsements.is_empty() {
                    self.channels
                        .pool_command_sender
                        .add_endorsements(endorsements)
                        .await?;
                }
            }
        }

        // add stale blocks to stats
        let new_stale_block_ids_creators_slots = self.block_db.get_new_stale_blocks();
        let timestamp = MassaTime::compensated_now(self.clock_compensation)?;
        for (b_id, (b_creator, b_slot)) in new_stale_block_ids_creators_slots.into_iter() {
            self.stale_block_stats.push_back(timestamp);

            let creator_addr = Address::from_public_key(&b_creator);
            if self.staking_keys.contains_key(&creator_addr) {
                warn!("block {} that was produced by our address {} at slot {} became stale. This is probably due to a temporary desynchronization.", b_id, creator_addr, b_slot);
            }
        }

        Ok(())
    }
}

/// A convenient function to create an endorsement
/// should probably be moved to models (or fully deleted)
pub fn create_endorsement(
    slot: Slot,
    sender_public_key: PublicKey,
    private_key: &PrivateKey,
    index: u32,
    endorsed_block: BlockId,
) -> Result<(EndorsementId, SignedEndorsement)> {
    let content = Endorsement {
        sender_public_key,
        slot,
        index,
        endorsed_block,
    };
    let (e_id, endorsement) = Signed::new_signed(content, private_key)?;
    Ok((e_id, endorsement))
}
