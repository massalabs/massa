// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::{ledger::LedgerData, pos::ExportProofOfStake};

use super::{
    block_graph::*, config::ConsensusConfig, error::ConsensusError, pos::ProofOfStake, timeslots::*,
};
use communication::protocol::{ProtocolCommandSender, ProtocolEvent, ProtocolEventReceiver};
use crypto::{
    hash::Hash,
    signature::{derive_public_key, PrivateKey, PublicKey},
};
use models::{
    Address, Amount, Block, BlockHeader, BlockHeaderContent, BlockId, Endorsement,
    EndorsementContent, EndorsementId, Operation, OperationId, OperationSearchResult,
    OperationSearchResultStatus, SerializeCompact, Slot, StakersCycleProductionStats,
};
use pool::PoolCommandSender;
use serde::{Deserialize, Serialize};
use std::{cmp::max, collections::VecDeque, convert::TryFrom};
use std::{
    collections::{HashMap, HashSet},
    usize,
};
use storage::StorageAccess;
use time::UTime;
use tokio::{
    sync::{mpsc, mpsc::error::SendTimeoutError, oneshot},
    time::{sleep_until, Sleep},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AddressState {
    pub final_rolls: u64,
    pub active_rolls: Option<u64>,
    pub candidate_rolls: u64,
    pub locked_balance: Amount,
    pub candidate_ledger_data: LedgerData,
    pub final_ledger_data: LedgerData,
}

/// Commands that can be proccessed by consensus.
#[derive(Debug)]
pub enum ConsensusCommand {
    /// Returns through a channel current blockgraph without block operations.
    GetBlockGraphStatus(oneshot::Sender<BlockGraphExport>),
    /// Returns through a channel full block with specified hash.
    GetActiveBlock {
        block_id: BlockId,
        response_tx: oneshot::Sender<Option<Block>>,
    },
    /// Returns through a channel full block and status with specified hash.
    GetBlockStatus {
        block_id: BlockId,
        response_tx: oneshot::Sender<Option<ExportBlockStatus>>,
    },
    /// Returns through a channel the list of slots with the address of the selected staker.
    GetSelectionDraws {
        start: Slot,
        end: Slot,
        response_tx: oneshot::Sender<Result<Vec<(Slot, (Address, Vec<Address>))>, ConsensusError>>,
    },
    /// Returns the bootstrap state
    GetBootstrapState(oneshot::Sender<(ExportProofOfStake, BootstrapableGraph)>),
    /// Returns info for a set of addresses (rolls and balance)
    GetAddressesInfo {
        addresses: HashSet<Address>,
        response_tx: oneshot::Sender<HashMap<Address, AddressState>>,
    },
    GetRecentOperations {
        address: Address,
        response_tx: oneshot::Sender<HashMap<OperationId, OperationSearchResult>>,
    },
    GetOperations {
        operation_ids: HashSet<OperationId>,
        response_tx: oneshot::Sender<HashMap<OperationId, OperationSearchResult>>,
    },
    GetStats(oneshot::Sender<ConsensusStats>),
    GetActiveStakers(oneshot::Sender<Option<HashMap<Address, u64>>>),
    RegisterStakingPrivateKeys(Vec<PrivateKey>),
    RemoveStakingAddresses(HashSet<Address>),
    GetStakingAddressses(oneshot::Sender<HashSet<Address>>),
    GetStakersProductionStats {
        addrs: HashSet<Address>,
        response_tx: oneshot::Sender<Vec<StakersCycleProductionStats>>,
    },
    GetBlockIdsByCreator {
        address: Address,
        response_tx: oneshot::Sender<HashMap<BlockId, Status>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusStats {
    timespan: UTime,
    final_block_count: u64,
    final_operation_count: u64,
    stale_block_count: u64,
    clique_count: u64,
}

/// Events that are emitted by consensus.
#[derive(Debug, Clone)]
pub enum ConsensusEvent {
    NeedSync, // probable desync detected, need resync
}

/// Events that are emitted by consensus.
#[derive(Debug, Clone)]
pub enum ConsensusManagementCommand {}

/// Manages consensus.
pub struct ConsensusWorker {
    /// Consensus Configuration
    cfg: ConsensusConfig,
    /// Genesis blocks werecreated with that public key.
    genesis_public_key: PublicKey,
    /// Associated protocol command sender.
    protocol_command_sender: ProtocolCommandSender,
    /// Associated protocol event listener.
    protocol_event_receiver: ProtocolEventReceiver,
    /// Associated Pool command sender.
    pool_command_sender: PoolCommandSender,
    /// Associated storage command sender. If we want to have long term final blocks storage.
    opt_storage_command_sender: Option<StorageAccess>,
    /// Database containing all information about blocks, the blockgraph and cliques.
    block_db: BlockGraph,
    /// Channel receiving consensus commands.
    controller_command_rx: mpsc::Receiver<ConsensusCommand>,
    /// Channel sending out consensus events.
    controller_event_tx: mpsc::Sender<ConsensusEvent>,
    /// Channel receiving consensus management commands.
    controller_manager_rx: mpsc::Receiver<ConsensusManagementCommand>,
    /// Proof of stake module.
    pos: ProofOfStake,
    /// Previous slot.
    previous_slot: Option<Slot>,
    /// Next slot
    next_slot: Slot,
    /// blocks we want
    wishlist: HashSet<BlockId>,
    // latest final periods
    latest_final_periods: Vec<u64>,
    /// clock compensation
    clock_compensation: i64,
    // staking keys
    staking_keys: HashMap<Address, (PublicKey, PrivateKey)>,
    // stats (block -> tx_count, creator)
    final_block_stats: VecDeque<(UTime, u64, Address)>,
    stale_block_stats: VecDeque<UTime>,
    stats_history_timespan: UTime,
    stats_desync_detection_timespan: UTime,
    launch_time: UTime,
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
    pub async fn new(
        cfg: ConsensusConfig,
        protocol_command_sender: ProtocolCommandSender,
        protocol_event_receiver: ProtocolEventReceiver,
        pool_command_sender: PoolCommandSender,
        opt_storage_command_sender: Option<StorageAccess>,
        block_db: BlockGraph,
        pos: ProofOfStake,
        controller_command_rx: mpsc::Receiver<ConsensusCommand>,
        controller_event_tx: mpsc::Sender<ConsensusEvent>,
        controller_manager_rx: mpsc::Receiver<ConsensusManagementCommand>,
        clock_compensation: i64,
        staking_keys: HashMap<Address, (PublicKey, PrivateKey)>,
    ) -> Result<ConsensusWorker, ConsensusError> {
        let previous_slot = get_current_latest_block_slot(
            cfg.thread_count,
            cfg.t0,
            cfg.genesis_timestamp,
            clock_compensation,
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
            UTime::now(clock_compensation)?.to_utc_string(),
            next_slot.get_cycle(cfg.periods_per_cycle),
            next_slot.period,
            next_slot.thread,
        );
        for addr in staking_keys.keys() {
            info!("Staking enabled for address: {}", addr);
        }
        massa_trace!("consensus.consensus_worker.new", {});

        // add genesis blocks to stats
        let genesis_public_key = derive_public_key(&cfg.genesis_key);
        let genesis_addr = Address::from_public_key(&genesis_public_key)?;
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

        Ok(ConsensusWorker {
            genesis_public_key,
            protocol_command_sender,
            protocol_event_receiver,
            opt_storage_command_sender,
            block_db,
            controller_command_rx,
            controller_event_tx,
            controller_manager_rx,
            pos,
            previous_slot,
            next_slot,
            wishlist: HashSet::new(),
            latest_final_periods,
            clock_compensation,
            pool_command_sender,
            staking_keys,
            final_block_stats,
            stale_block_stats: VecDeque::new(),
            stats_desync_detection_timespan,
            stats_history_timespan: max(stats_desync_detection_timespan, cfg.stats_timespan),
            cfg,
            launch_time: UTime::now(clock_compensation)?,
            endorsed_slots: HashSet::new(),
        })
    }

    /// Consensus work is managed here.
    /// It's mostly a tokio::select within a loop.
    pub async fn run_loop(mut self) -> Result<ProtocolEventReceiver, ConsensusError> {
        // signal initial state to pool
        if let Some(previous_slot) = self.previous_slot {
            self.pool_command_sender
                .update_current_slot(previous_slot)
                .await?;
        }
        self.pool_command_sender
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

        loop {
            massa_trace!("consensus.consensus_worker.run_loop.select", {});
            tokio::select! {
                // slot timer
                _ = &mut next_slot_timer => {
                    massa_trace!("consensus.consensus_worker.run_loop.select.slot_tick", {});
                    if let Some(end) = self.cfg.end_timestamp {
                        if UTime::now(self.clock_compensation)? > end {
                            info!("This episode has come to an end, please get the latest testnet node version to continue");
                            break;
                        }
                    }
                    self.slot_tick(&mut next_slot_timer).await?;
                },

                // listen consensus commands
                Some(cmd) = self.controller_command_rx.recv() => {
                    massa_trace!("consensus.consensus_worker.run_loop.consensus_command", {});
                    self.process_consensus_command(cmd).await?},

                // receive protocol controller events
                evt = self.protocol_event_receiver.wait_event() =>{
                    massa_trace!("consensus.consensus_worker.run_loop.select.protocol_event", {});
                    match evt {
                        Ok(event) => self.process_protocol_event(event).await?,
                        Err(err) => return Err(ConsensusError::CommunicationError(err))
                    }
                },

                // listen to manager commands
                cmd = self.controller_manager_rx.recv() => {
                    massa_trace!("consensus.consensus_worker.run_loop.select.manager", {});
                    match cmd {
                    None => break,
                    Some(_) => {}
                }}
            }
        }
        // end loop
        Ok(self.protocol_event_receiver)
    }

    async fn slot_tick(
        &mut self,
        next_slot_timer: &mut std::pin::Pin<&mut Sleep>,
    ) -> Result<(), ConsensusError> {
        let now = UTime::now(self.clock_compensation)?;
        let cur_slot = self.next_slot;
        self.previous_slot = Some(cur_slot);
        self.next_slot = self.next_slot.get_next_slot(self.cfg.thread_count)?;
        massa_trace!("consensus.consensus_worker.slot_tick", { "slot": cur_slot });
        let cur_cycle = self.next_slot.get_cycle(self.cfg.periods_per_cycle);

        if let Some(slot) = self.previous_slot {
            if slot.get_cycle(self.cfg.periods_per_cycle) != cur_cycle {
                info!("Started cycle {}", cur_cycle);
            }
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
        self.pool_command_sender
            .update_current_slot(cur_slot)
            .await?;

        // create blocks
        if !self.cfg.disable_block_creation && cur_slot.period > 0 {
            let block_draw = match self.pos.draw_block_producer(cur_slot) {
                Ok(b_draw) => Some(b_draw),
                Err(ConsensusError::PosCycleUnavailable(_)) => {
                    massa_trace!(
                        "consensus.consensus_worker.slot_tick.block_creatorunavailable",
                        {}
                    );
                    warn!("desynchronization detected because the lookback cycle is not final at the current time");
                    let _ = self.send_consensus_event(ConsensusEvent::NeedSync).await;
                    None
                }
                Err(err) => return Err(err),
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
        }

        // signal tick to block graph
        self.block_db.slot_tick(&mut self.pos, Some(cur_slot))?;

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

    async fn create_block(
        &mut self,
        cur_slot: Slot,
        creator_addr: &Address,
        creator_public_key: &PublicKey,
        creator_private_key: &PrivateKey,
    ) -> Result<(), ConsensusError> {
        // get parents
        let parents = self.block_db.get_best_parents();
        let (thread_parent, thread_parent_period) = parents[cur_slot.thread as usize];

        // get endorsements
        let endorsements = if thread_parent_period > 0 {
            let thread_parent_slot = Slot::new(thread_parent_period, cur_slot.thread);
            let endorsement_draws = self.pos.draw_endorsement_producers(thread_parent_slot)?;
            self.pool_command_sender
                .get_endorsements(thread_parent_slot, thread_parent, endorsement_draws)
                .await?
        } else {
            Vec::new()
        };

        massa_trace!("consensus.create_block.get_endorsements.result", {
            "endorsements": endorsements
        });

        // create empty block
        let (_block_id, header) = BlockHeader::new_signed(
            creator_private_key,
            BlockHeaderContent {
                creator: *creator_public_key,
                slot: cur_slot,
                parents: parents.iter().map(|(b, _p)| *b).collect(),
                operation_merkle_root: Hash::hash(&Vec::new()[..]),
                endorsements: endorsements.clone(),
            },
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
        let mut exclude_operations: HashSet<OperationId> = HashSet::new();
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
                        "missing ancestor to check operation reuse for block creation: {:?}",
                        ancestor_id
                    ))
                })?;
            if ancestor.block.header.content.slot.period < stop_period {
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
        let mut operations: Vec<Operation> = Vec::new();
        let mut operation_set: HashMap<OperationId, (usize, u64)> = HashMap::new(); // (index, validity end period)
        let mut finished = remaining_block_space == 0
            || remaining_operation_count == 0
            || self.cfg.max_operations_fill_attempts == 0;
        let mut attempts = 0;
        while !finished {
            // get a batch of operations
            let operation_batch = self
                .pool_command_sender
                .get_operation_batch(
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
                total_hash.extend(op_id.to_bytes());

                // check if the block still has some space
                if remaining_block_space == 0 || remaining_operation_count == 0 {
                    finished = true;
                    break;
                }
            }
        }

        // compile resulting block
        let (block_id, header) = BlockHeader::new_signed(
            creator_private_key,
            BlockHeaderContent {
                creator: *creator_public_key,
                slot: cur_slot,
                parents: parents.iter().map(|(b, _p)| *b).collect(),
                operation_merkle_root: Hash::hash(&total_hash),
                endorsements,
            },
        )?;
        let block = Block { header, operations };

        massa_trace!("create block", { "block": block });
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
            block,
            operation_set,
            &mut self.pos,
            Some(cur_slot),
        )?;

        Ok(())
    }

    async fn send_consensus_event(&self, event: ConsensusEvent) -> Result<(), ConsensusError> {
        let result = self
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
    ///
    /// # Argument
    /// * cmd: consens command to process
    async fn process_consensus_command(
        &mut self,
        cmd: ConsensusCommand,
    ) -> Result<(), ConsensusError> {
        match cmd {
            ConsensusCommand::GetBlockGraphStatus(response_tx) => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_block_graph_status",
                    {}
                );
                response_tx
                    .send(BlockGraphExport::from(&self.block_db))
                    .map_err(|err| {
                        ConsensusError::SendChannelError(format!(
                            "could not send GetBlockGraphStatus answer:{:?}",
                            err
                        ))
                    })
            }
            //return full block with specified hash
            ConsensusCommand::GetActiveBlock {
                block_id,
                response_tx,
            } => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_active_block",
                    {}
                );
                response_tx
                    .send(
                        self.block_db
                            .get_active_block(&block_id)
                            .map(|v| v.block.clone()),
                    )
                    .map_err(|err| {
                        ConsensusError::SendChannelError(format!(
                            "could not send GetBlock answer:{:?}",
                            err
                        ))
                    })
            }
            //return full block and status with specified hash
            ConsensusCommand::GetBlockStatus {
                block_id,
                response_tx,
            } => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_block_status",
                    {}
                );

                let mut found_block = self.block_db.get_export_block_status(&block_id);
                if found_block.is_none() {
                    if let Some(storage) = &self.opt_storage_command_sender {
                        found_block = storage
                            .get_block(block_id)
                            .await?
                            .map(ExportBlockStatus::Stored);
                    }
                }

                response_tx.send(found_block).map_err(|err| {
                    ConsensusError::SendChannelError(format!(
                        "could not send GetBlock Status answer:{:?}",
                        err
                    ))
                })
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
                            match Address::from_public_key(&self.genesis_public_key) {
                                Ok(addr) => (addr, Vec::new()),
                                Err(err) => break Err(err.into()),
                            }
                        } else {
                            (
                                match self.pos.draw_block_producer(cur_slot) {
                                    Err(err) => break Err(err),
                                    Ok(block_addr) => block_addr,
                                },
                                match self.pos.draw_endorsement_producers(cur_slot) {
                                    Err(err) => break Err(err),
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
                response_tx.send(result).map_err(|err| {
                    ConsensusError::SendChannelError(format!(
                        "could not send GetSelectionDraws response: {:?}",
                        err
                    ))
                })
            }
            ConsensusCommand::GetBootstrapState(response_tx) => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_bootstrap_state",
                    {}
                );
                let resp = (
                    self.pos.export(),
                    BootstrapableGraph::try_from(&self.block_db)?,
                );
                response_tx.send(resp).map_err(|err| {
                    ConsensusError::SendChannelError(format!(
                        "could not send GetBootstrapState answer:{:?}",
                        err
                    ))
                })
            }
            ConsensusCommand::GetAddressesInfo {
                addresses,
                response_tx,
            } => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_addresses_info",
                    { "addresses": addresses }
                );
                response_tx
                    .send(self.get_addresses_info(&addresses)?)
                    .map_err(|err| {
                        ConsensusError::SendChannelError(format!(
                            "could not send GetAddressesInfo response: {:?}",
                            err
                        ))
                    })
            }
            ConsensusCommand::GetRecentOperations {
                address,
                response_tx,
            } => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_operations_involving_address",
                    { "address": address }
                );

                let mut res: HashMap<_, _> = self
                    .pool_command_sender
                    .get_operations_involving_address(address)
                    .await?;

                self.block_db
                    .get_operations_involving_address(&address)?
                    .into_iter()
                    .for_each(|(op_id, search_new)| {
                        res.entry(op_id)
                            .and_modify(|search_old| search_old.extend(&search_new))
                            .or_insert(search_new);
                    });

                if let Some(access) = &self.opt_storage_command_sender {
                    access
                        .get_operations_involving_address(&address)
                        .await?
                        .into_iter()
                        .for_each(|(op_id, search_new)| {
                            res.entry(op_id)
                                .and_modify(|search_old| search_old.extend(&search_new))
                                .or_insert(search_new);
                        })
                }

                response_tx.send(res).map_err(|err| {
                    ConsensusError::SendChannelError(format!(
                        "could not send GetRecentOPerations response: {:?}",
                        err
                    ))
                })
            }
            ConsensusCommand::GetOperations {
                operation_ids,
                response_tx,
            } => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_operations",
                    { "operation_ids": operation_ids }
                );
                let res = self.get_operations(&operation_ids).await?;
                response_tx.send(res).map_err(|err| {
                    ConsensusError::SendChannelError(format!(
                        "could not send get operations response: {:?}",
                        err
                    ))
                })
            }
            ConsensusCommand::GetStats(response_tx) => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_stats",
                    {}
                );
                let res = self.get_stats()?;
                response_tx.send(res).map_err(|err| {
                    ConsensusError::SendChannelError(format!(
                        "could not send get_stats response: {:?}",
                        err
                    ))
                })
            }
            ConsensusCommand::GetActiveStakers(response_tx) => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_active_stakers",
                    {}
                );
                let res = self.get_active_stakers()?;
                response_tx.send(res).map_err(|err| {
                    ConsensusError::SendChannelError(format!(
                        "could not send get_active_stakers response: {:?}",
                        err
                    ))
                })
            }
            ConsensusCommand::RegisterStakingPrivateKeys(keys) => {
                for key in keys.into_iter() {
                    let public = crypto::derive_public_key(&key);
                    let address = Address::from_public_key(&public)?;
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
                response_tx
                    .send(self.staking_keys.keys().cloned().collect())
                    .map_err(|err| {
                        ConsensusError::SendChannelError(format!(
                            "could not send get_staking addresses response: {:?}",
                            err
                        ))
                    })
            }
            ConsensusCommand::GetStakersProductionStats { addrs, response_tx } => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_stakers_production_stats",
                    {}
                );
                response_tx
                    .send(self.pos.get_stakers_production_stats(addrs))
                    .map_err(|err| {
                        ConsensusError::SendChannelError(format!(
                            "could not send get_staking addresses response: {:?}",
                            err
                        ))
                    })
            }
            ConsensusCommand::GetBlockIdsByCreator {
                address,
                response_tx,
            } => response_tx
                .send(self.block_db.get_block_ids_by_creator(&address))
                .map_err(|err| {
                    ConsensusError::SendChannelError(format!(
                        "could not send get block ids by creator response: {:?}",
                        err
                    ))
                }),
        }
    }

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

    fn get_stats(&mut self) -> Result<ConsensusStats, ConsensusError> {
        let timespan_end = max(self.launch_time, UTime::now(self.clock_compensation)?);
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
            timespan: timespan_end.saturating_sub(timespan_start),
            final_block_count,
            final_operation_count,
            stale_block_count,
            clique_count,
        })
    }

    fn get_active_stakers(&self) -> Result<Option<HashMap<Address, u64>>, ConsensusError> {
        let cur_cycle = self.next_slot.get_cycle(self.cfg.periods_per_cycle);
        let mut res: HashMap<Address, u64> = HashMap::new();
        for thread in 0..self.cfg.thread_count {
            match self.pos.get_lookback_roll_count(cur_cycle, thread) {
                Ok(rolls) => {
                    res.extend(&rolls.0);
                }
                Err(ConsensusError::PosCycleUnavailable(_)) => return Ok(None),
                Err(err) => return Err(err),
            }
        }
        Ok(Some(res))
    }

    fn get_addresses_info(
        &self,
        addresses: &HashSet<Address>,
    ) -> Result<HashMap<Address, AddressState>, ConsensusError> {
        let thread_count = self.cfg.thread_count;
        let mut addresses_by_thread = vec![HashSet::new(); thread_count as usize];
        for addr in addresses.iter() {
            addresses_by_thread[addr.get_thread(thread_count) as usize].insert(*addr);
        }
        let mut states = HashMap::new();
        let cur_cycle = self.next_slot.get_cycle(self.cfg.periods_per_cycle);
        let ledger_data = self.block_db.get_ledger_data_export(addresses)?;
        for thread in 0..thread_count {
            let lookback_data = match self.pos.get_lookback_roll_count(cur_cycle, thread) {
                Ok(rolls) => Some(rolls),
                Err(ConsensusError::PosCycleUnavailable(_)) => None,
                Err(e) => return Err(e),
            };
            let final_data = self
                .pos
                .get_final_roll_data(self.pos.get_last_final_block_cycle(thread), thread)
                .ok_or_else(|| {
                    ConsensusError::PosCycleUnavailable("final cycle unavailable".to_string())
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
                        final_rolls: *final_data.roll_count.0.get(addr).unwrap_or(&0),
                        active_rolls: lookback_data.map(|data| *data.0.get(addr).unwrap_or(&0)),
                        candidate_rolls: *candidate_data.0 .0.get(addr).unwrap_or(&0),
                        locked_balance: self
                            .cfg
                            .roll_price
                            .checked_mul_u64(*locked_rolls.get(addr).unwrap_or(&0))
                            .ok_or(ConsensusError::RollOverflowError)?,
                        candidate_ledger_data: ledger_data
                            .candidate_data
                            .0
                            .get(&addr)
                            .map_or_else(|| LedgerData::default(), |v| v.clone()),
                        final_ledger_data: ledger_data
                            .final_data
                            .0
                            .get(&addr)
                            .map_or_else(|| LedgerData::default(), |v| v.clone()),
                    },
                );
            }
        }
        Ok(states)
    }

    async fn get_operations(
        &mut self,
        operation_ids: &HashSet<OperationId>,
    ) -> Result<HashMap<OperationId, OperationSearchResult>, ConsensusError> {
        // get from pool
        let mut res: HashMap<OperationId, OperationSearchResult> = self
            .pool_command_sender
            .get_operations(operation_ids.clone())
            .await?
            .into_iter()
            .map(|(op_id, op)| {
                (
                    op_id,
                    OperationSearchResult {
                        op,
                        in_pool: true,
                        in_blocks: HashMap::new(),
                        status: OperationSearchResultStatus::Pending,
                    },
                )
            })
            .collect();

        // extend with consensus
        self.block_db
            .get_operations(operation_ids)
            .into_iter()
            .for_each(|(op_id, search_new)| {
                res.entry(op_id)
                    .and_modify(|search_old| search_old.extend(&search_new))
                    .or_insert(search_new);
            });

        // for those that have not been found in consensus, extend with storage
        if let Some(storage) = &mut self.opt_storage_command_sender {
            let to_gather: HashSet<OperationId> = operation_ids
                .iter()
                .filter(|op_id| {
                    if let Some(cur_search) = res.get(op_id) {
                        if !cur_search.in_blocks.is_empty() {
                            return false;
                        }
                    }
                    true
                })
                .copied()
                .collect();
            storage
                .get_operations(to_gather)
                .await?
                .into_iter()
                .for_each(|(op_id, search_new)| {
                    res.entry(op_id)
                        .and_modify(|search_old| search_old.extend(&search_new))
                        .or_insert(search_new);
                });
        }

        Ok(res)
    }

    /// Manages received protocolevents.
    ///
    /// # Arguments
    /// * event: event type to process.
    async fn process_protocol_event(&mut self, event: ProtocolEvent) -> Result<(), ConsensusError> {
        match event {
            ProtocolEvent::ReceivedBlock {
                block_id,
                block,
                operation_set,
            } => {
                massa_trace!("consensus.consensus_worker.process_protocol_event.received_block", { "block_id": block_id, "block": block });
                self.block_db.incoming_block(
                    block_id,
                    block,
                    operation_set,
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
                let mut results = HashMap::new();
                for block_hash in list {
                    if let Some(a_block) = self.block_db.get_active_block(&block_hash) {
                        massa_trace!("consensus.consensus_worker.process_protocol_event.get_block.consensus_found", { "hash": block_hash});
                        results.insert(block_hash, Some(a_block.block.clone()));
                    } else if let Some(storage_command_sender) = &self.opt_storage_command_sender {
                        if let Some(block) = storage_command_sender.get_block(block_hash).await? {
                            massa_trace!("consensus.consensus_worker.process_protocol_event.get_block.storage_found", { "hash": block_hash});
                            results.insert(block_hash, Some(block));
                        } else {
                            // not found in given storage
                            massa_trace!("consensus.consensus_worker.process_protocol_event.get_block.storage_not_found", { "hash": block_hash});
                            results.insert(block_hash, None);
                        }
                    } else {
                        // not found in consensus and no storage provided
                        massa_trace!("consensus.consensus_worker.process_protocol_event.get_block.consensu_not_found", { "hash": block_hash});
                        results.insert(block_hash, None);
                    }
                }
                self.protocol_command_sender
                    .send_get_blocks_results(results)
                    .await?;
            }
        }
        Ok(())
    }

    // prune statistics
    fn prune_stats(&mut self) -> Result<(), ConsensusError> {
        let start_time =
            UTime::now(self.clock_compensation)?.saturating_sub(self.stats_history_timespan);
        self.final_block_stats.retain(|(t, _, _)| t >= &start_time);
        self.stale_block_stats.retain(|t| t >= &start_time);
        Ok(())
    }

    async fn block_db_changed(&mut self) -> Result<(), ConsensusError> {
        massa_trace!("consensus.consensus_worker.block_db_changed", {});

        // Propagate new blocks
        for (block_id, block) in self.block_db.get_blocks_to_propagate().into_iter() {
            massa_trace!("consensus.consensus_worker.block_db_changed.integrated", { "block_id": block_id, "block": block });
            self.protocol_command_sender
                .integrated_block(block_id, block)
                .await?;
        }

        // Notify protocol of attack attempts.
        for hash in self.block_db.get_attack_attempts().into_iter() {
            self.protocol_command_sender
                .notify_block_attack(hash)
                .await?;
            massa_trace!("consensus.consensus_worker.block_db_changed.attack", {
                "hash": hash
            });
        }

        // Process new final blocks
        let new_final_block_ids = self.block_db.get_new_final_blocks();
        let mut new_final_ops: HashMap<OperationId, (u64, u8)> = HashMap::new();
        let mut new_final_blocks = HashMap::with_capacity(new_final_block_ids.len());
        let timestamp = UTime::now(self.clock_compensation)?;
        for b_id in new_final_block_ids.into_iter() {
            if let Some(a_block) = self.block_db.get_active_block(&b_id) {
                // List new final ops
                new_final_ops.extend(
                    a_block.operation_set.iter().map(|(id, (_, exp))| {
                        (*id, (*exp, a_block.block.header.content.slot.thread))
                    }),
                );
                // List final block
                new_final_blocks.insert(b_id, a_block);
                // add to stats
                self.final_block_stats.push_back((
                    timestamp,
                    a_block.operation_set.len() as u64,
                    Address::from_public_key(&a_block.block.header.content.creator)?,
                ));
            }
        }
        // Notify pool of new final ops
        if !new_final_ops.is_empty() {
            self.pool_command_sender
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
            self.protocol_command_sender
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
            self.pool_command_sender
                .update_latest_final_periods(self.latest_final_periods.clone())
                .await?;
        }

        // Produce endorsements
        if !self.cfg.disable_block_creation {
            // iterate on all blockclique blocks
            for block_id in self.block_db.get_blockclique().into_iter() {
                let block_slot = match self.block_db.get_active_block(&block_id) {
                    Some(b) => b.block.header.content.slot,
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
                let mut endorsements = HashMap::new();
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
                    self.pool_command_sender
                        .add_endorsements(endorsements)
                        .await?;
                }
            }
        }

        // add stale blocks to stats
        let new_stale_block_ids_slots = self.block_db.get_new_stale_blocks();
        let timestamp = UTime::now(self.clock_compensation)?;
        for (_b_id, _b_slot) in new_stale_block_ids_slots.into_iter() {
            self.stale_block_stats.push_back(timestamp);
        }

        // prune block db and send discarded final blocks to storage if present
        let discarded_final_blocks = self.block_db.prune()?;
        if let Some(storage_cmd) = &self.opt_storage_command_sender {
            storage_cmd.add_block_batch(discarded_final_blocks).await?;
        }

        Ok(())
    }
}

pub fn create_endorsement(
    slot: Slot,
    sender_public_key: PublicKey,
    private_key: &PrivateKey,
    index: u32,
    endorsed_block: BlockId,
) -> Result<(EndorsementId, Endorsement), ConsensusError> {
    let content = EndorsementContent {
        sender_public_key,
        slot,
        index,
        endorsed_block,
    };
    let (e_id, endorsement) = Endorsement::new_signed(private_key, content)?;
    Ok((e_id, endorsement))
}
