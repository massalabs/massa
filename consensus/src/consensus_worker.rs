use crate::ledger::{LedgerChange, LedgerSubset, OperationLedgerInterface};

use super::{
    block_graph::*, config::ConsensusConfig, error::ConsensusError, random_selector::*,
    timeslots::*,
};
use communication::protocol::{ProtocolCommandSender, ProtocolEvent, ProtocolEventReceiver};
use crypto::{
    hash::Hash,
    signature::{derive_public_key, PublicKey},
};
use models::{
    Address, Block, BlockHeader, BlockHeaderContent, BlockId, Operation, OperationId,
    SerializationContext, SerializeCompact, Slot,
};
use pool::PoolCommandSender;
use std::convert::{TryFrom, TryInto};
use std::{
    collections::{HashMap, HashSet},
    usize,
};
use storage::StorageAccess;
use tokio::{
    sync::{mpsc, oneshot},
    time::{sleep_until, Sleep},
};

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
    /// Returns through a channel the list of slots with public key of the selected staker.
    GetSelectionDraws {
        start: Slot,
        end: Slot,
        response_tx: oneshot::Sender<Result<Vec<(Slot, PublicKey)>, ConsensusError>>,
    },
    /// Returns the bootstrap graph
    GetBootGraph(oneshot::Sender<BootsrapableGraph>),
    /// Returns through a channel current blockgraph without block operations.
    GetLedgerData {
        addresses: HashSet<Address>,
        response_tx: oneshot::Sender<LedgerDataExport>,
    },
}

/// Events that are emitted by consensus.
#[derive(Debug, Clone)]
pub enum ConsensusEvent {}

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
    _controller_event_tx: mpsc::Sender<ConsensusEvent>,
    /// Channel receiving consensus management commands.
    controller_manager_rx: mpsc::Receiver<ConsensusManagementCommand>,
    /// Selector used to select roll numbers.
    selector: RandomSelector,
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
    /// serialisation context
    serialization_context: SerializationContext,
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
    pub fn new(
        cfg: ConsensusConfig,
        protocol_command_sender: ProtocolCommandSender,
        protocol_event_receiver: ProtocolEventReceiver,
        pool_command_sender: PoolCommandSender,
        opt_storage_command_sender: Option<StorageAccess>,
        block_db: BlockGraph,
        controller_command_rx: mpsc::Receiver<ConsensusCommand>,
        controller_event_tx: mpsc::Sender<ConsensusEvent>,
        controller_manager_rx: mpsc::Receiver<ConsensusManagementCommand>,
        clock_compensation: i64,
        serialization_context: SerializationContext,
    ) -> Result<ConsensusWorker, ConsensusError> {
        let seed = vec![0u8; 32]; // TODO temporary (see issue #103)
        let participants_weights = vec![1u64; cfg.nodes.len()]; // TODO (see issue #104)
        let selector = RandomSelector::new(&seed, cfg.thread_count, participants_weights)?;
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

        massa_trace!("consensus.consensus_worker.new", {});
        let genesis_public_key = derive_public_key(&cfg.genesis_key);
        Ok(ConsensusWorker {
            cfg: cfg.clone(),
            genesis_public_key,
            protocol_command_sender,
            protocol_event_receiver,
            opt_storage_command_sender,
            block_db,
            controller_command_rx,
            _controller_event_tx: controller_event_tx,
            controller_manager_rx,
            selector,
            previous_slot,
            next_slot,
            wishlist: HashSet::new(),
            latest_final_periods,
            clock_compensation,
            pool_command_sender,
            serialization_context,
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
                    self.slot_tick(&mut next_slot_timer).await?
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

    async fn get_best_operations(
        &mut self,
        cur_slot: Slot,
        mut left_size: u64,
    ) -> Result<Vec<Operation>, ConsensusError> {
        let mut ops = Vec::new();
        let mut excluded: Vec<OperationId> = Vec::new();
        let mut ids_to_keep: Vec<OperationId> = Vec::new();

        let fee_target = Address::from_public_key(
            &self
                .cfg
                .nodes
                .get(self.cfg.current_node_index as usize)
                .and_then(|(public_key, private_key)| {
                    Some((public_key.clone(), private_key.clone()))
                })
                .ok_or(ConsensusError::KeyError)?
                .0,
        )?;

        let mut query_addr = HashSet::new();
        if fee_target.get_thread(self.cfg.thread_count) == cur_slot.thread {
            query_addr.insert(fee_target);
        }

        let asked_nb = self.cfg.operation_batch_size;
        let mut ledger = self
            .block_db
            .get_ledger_at_parents(&self.block_db.get_best_parents(), &query_addr)?;

        if fee_target.get_thread(self.cfg.thread_count) == cur_slot.thread {
            // reward
            let reward_ledger_change = LedgerChange {
                balance_delta: self.cfg.block_reward,
                balance_increment: true,
            };
            let reward_change = (&fee_target, &reward_ledger_change);

            ledger.apply_change(reward_change, self.cfg.thread_count)?;
        }

        excluded.extend(self.block_db.get_past_operations(
            cur_slot,
            &self.block_db.get_best_parents()[cur_slot.thread as usize],
        )?);

        while ops.len() < self.cfg.max_operations_per_block as usize {
            let to_exclude = [excluded.clone(), ids_to_keep.clone()].concat();
            let (response_tx, response_rx) = oneshot::channel();
            self.pool_command_sender
                .get_operation_batch(
                    cur_slot,
                    to_exclude.iter().cloned().collect(),
                    asked_nb,
                    left_size,
                    response_tx,
                )
                .await?;

            let candidates = response_rx.await?;

            // operations are already sorted by interest
            let involved_addresses: HashSet<Address> = candidates
                .iter()
                .map(|(_, op, size)| op.get_involved_addresses(&fee_target))
                .flatten()
                .flatten()
                .filter(|a| a.get_thread(self.cfg.thread_count) == cur_slot.thread)
                .collect();

            // if new addresses are needed, update ledger
            ledger.merge(
                self.block_db.get_ledger_at_parents(
                    &self.block_db.get_best_parents(),
                    &involved_addresses,
                )?,
            );

            let mut cpt = 0;
            for (id, op, size) in candidates.iter() {
                if *size > left_size {
                    cpt += 1;
                    continue;
                }
                let changes = op.get_changes(&fee_target, self.cfg.thread_count)?;
                match ledger.try_apply_changes(changes) {
                    Ok(_) => {
                        // that operation can be included
                        ops.push(op.clone());
                        ids_to_keep.push(*id);
                        left_size = left_size - size;
                    }
                    Err(e) => {
                        // Something went wrong with that op, exclude it
                        info!("operation {:?} was rejected due to {:?}", id, e);
                        excluded.push(*id);
                    }
                }
            }
            // break if every operation was rejected due to size
            if cpt == candidates.len() {
                break;
            }

            // no operations left to select
            if candidates.len() < asked_nb {
                break;
            }
        }

        Ok(ops)
    }

    async fn slot_tick(
        &mut self,
        next_slot_timer: &mut std::pin::Pin<&mut Sleep>,
    ) -> Result<(), ConsensusError> {
        let cur_slot = self.next_slot;
        self.previous_slot = Some(cur_slot);
        self.next_slot = self.next_slot.get_next_slot(self.cfg.thread_count)?;

        massa_trace!("consensus.consensus_worker.slot_tick", { "slot": cur_slot });

        let block_creator = self.selector.draw(cur_slot);

        // create a block if enabled and possible
        if !self.cfg.disable_block_creation
            && self.next_slot.period > 0
            && block_creator == self.cfg.current_node_index
        {
            self.create_block(cur_slot).await?;
        }

        // signal tick to block graph
        self.block_db
            .slot_tick(&mut self.selector, Some(cur_slot))?;

        // signal tick to pool
        self.pool_command_sender
            .update_current_slot(cur_slot)
            .await?;

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

        Ok(())
    }

    async fn create_block(&mut self, cur_slot: Slot) -> Result<(), ConsensusError> {
        // get creator info
        // TODO temporary: use PoS draw system in the future
        let (creator_pub, creator_priv) = self
            .cfg
            .nodes
            .get(self.cfg.current_node_index as usize)
            .ok_or(ConsensusError::KeyError)?;
        let creator_addr = Address::from_public_key(creator_pub)?;

        // get parents
        let parents = self.block_db.get_best_parents();

        // create empty block
        let (_block_id, header) = BlockHeader::new_signed(
            creator_priv,
            BlockHeaderContent {
                creator: *creator_pub,
                slot: cur_slot,
                parents: parents.clone(),
                operation_merkle_root: Hash::hash(&Vec::new()[..]),
            },
            &self.serialization_context,
        )?;
        let block = Block {
            header,
            operations: Vec::new(),
        };

        // initialize remaining block space and remaining operation count
        let mut remaining_block_space = (self.cfg.max_block_size as u64)
            .checked_sub(block.to_bytes_compact(&self.serialization_context)?.len() as u64)
            .ok_or(ConsensusError::BlockCreationError(
                "consensus config max_block_size is smaller than an ampty block".into(),
            ))?;
        let mut remaining_operation_count = self.cfg.max_operations_per_block as usize;

        // exclude operations that were used in block ancestry
        let mut exclude_operations: HashSet<OperationId> = HashSet::new();
        let mut ancestor_id = block.header.content.parents[cur_slot.thread as usize];
        let stop_period = cur_slot
            .period
            .saturating_sub(self.cfg.operation_validity_periods);
        loop {
            let ancestor = self.block_db.get_active_block(&ancestor_id).ok_or(
                ConsensusError::ContainerInconsistency(format!(
                    "missing ancestor to check operation reuse for block creation: {:?}",
                    ancestor_id
                )),
            )?;
            if ancestor.block.header.content.slot.period < stop_period {
                break;
            }
            exclude_operations.extend(&ancestor.operation_set);
            if ancestor.parents.is_empty() {
                break;
            }
            ancestor_id = ancestor.parents[cur_slot.thread as usize].0;
        }

        // get thread ledger and apply block reward
        let mut thread_ledger = LedgerSubset::new(self.cfg.thread_count);
        if creator_addr.get_thread(self.cfg.thread_count) == cur_slot.thread {
            let mut fee_ledger = self
                .block_db
                .get_ledger_at_parents(&parents, &vec![creator_addr].into_iter().collect())?;
            fee_ledger.apply_change((
                &creator_addr,
                &LedgerChange {
                    balance_delta: self.cfg.block_reward,
                    balance_increment: true,
                },
            ))?;
            thread_ledger.extend(fee_ledger);
        }

        // gather operations
        let mut total_hash: Vec<u8> = Vec::new();
        let mut operations: Vec<Operation> = Vec::new();
        let mut operation_set: HashSet<OperationId> = HashSet::new();
        let mut finished = remaining_block_space == 0 || remaining_operation_count == 0;
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
            finished = operation_batch.len() < self.cfg.operation_batch_size;

            for (op_id, op, op_size) in operation_batch.into_iter() {
                // exclude operation from future batches
                exclude_operations.insert(op_id);

                // check that the operation fits in size
                if op_size > remaining_block_space {
                    continue;
                }

                // try update thread ledger
                let mut thread_changes: Vec<HashMap<Address, LedgerChange>> =
                    vec![HashMap::new(); self.cfg.thread_count as usize];
                thread_changes[cur_slot.thread as usize] = op
                    .get_changes(&creator_addr, self.cfg.thread_count)?
                    .swap_remove(cur_slot.thread as usize);
                if thread_ledger.try_apply_changes(&thread_changes).is_err() {
                    continue;
                }

                // add operation
                operations.push(op);
                operation_set.insert(op_id);
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
            creator_priv,
            BlockHeaderContent {
                creator: *creator_pub,
                slot: cur_slot,
                parents: parents.clone(),
                operation_merkle_root: Hash::hash(&total_hash),
            },
            &self.serialization_context,
        )?;
        let block = Block { header, operations };

        // add block to db
        self.block_db.incoming_block(
            block_id,
            block,
            operation_set,
            &mut self.selector,
            Some(cur_slot),
        )?;

        Ok(())
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
                            self.genesis_public_key
                        } else {
                            self.cfg.nodes[self.selector.draw(cur_slot) as usize].0
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
            ConsensusCommand::GetBootGraph(response_tx) => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_boot_graph",
                    {}
                );
                response_tx
                    .send(BootsrapableGraph::try_from(&self.block_db)?)
                    .map_err(|err| {
                        ConsensusError::SendChannelError(format!(
                            "could not send GetBootGraph answer:{:?}",
                            err
                        ))
                    })
            }
            ConsensusCommand::GetLedgerData {
                addresses,
                response_tx,
            } => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_ledger_data",
                    { "addresses": addresses }
                );
                let best_parents = self.block_db.get_best_parents();
                let last_final_block_ids: Vec<BlockId> = self
                    .block_db
                    .get_latest_final_blocks_periods()
                    .iter()
                    .map(|(b, _p)| *b)
                    .collect();
                let result = LedgerDataExport {
                    candidate_data: self
                        .block_db
                        .get_ledger_at_parents(best_parents, &addresses)?,
                    final_data: self
                        .block_db
                        .get_ledger_at_parents(&last_final_block_ids, &addresses)?,
                };
                response_tx.send(result).map_err(|err| {
                    ConsensusError::SendChannelError(format!(
                        "could not send GetLedgerData response: {:?}",
                        err
                    ))
                })
            }
        }
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
                    &mut self.selector,
                    self.previous_slot,
                )?;
                self.block_db_changed().await?;
            }
            ProtocolEvent::ReceivedBlockHeader { block_id, header } => {
                massa_trace!("consensus.consensus_worker.process_protocol_event.received_header", { "block_id": block_id, "header": header });
                self.block_db.incoming_header(
                    block_id,
                    header,
                    &mut self.selector,
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

    async fn block_db_changed(&mut self) -> Result<(), ConsensusError> {
        massa_trace!("consensus.consensus_worker.block_db_changed", {});
        // prune block db and send discarded final blocks to storage if present
        let discarded_final_blocks = self.block_db.prune()?;
        if let Some(storage_cmd) = &self.opt_storage_command_sender {
            storage_cmd.add_block_batch(discarded_final_blocks).await?;
        }

        // Propagate newly active blocks.
        for (hash, block) in self.block_db.get_blocks_to_propagate().into_iter() {
            massa_trace!("consensus.consensus_worker.block_db_changed.integrated", { "hash": hash, "block": block });
            self.protocol_command_sender
                .integrated_block(hash, block)
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

        // send latest final periods to pool, if changed
        let latest_final_periods: Vec<u64> = self
            .block_db
            .get_latest_final_blocks_periods()
            .iter()
            .map(|(_block_id, period)| *period)
            .collect();
        if self.latest_final_periods != latest_final_periods {
            self.latest_final_periods = latest_final_periods;
            self.pool_command_sender
                .update_latest_final_periods(self.latest_final_periods.clone())
                .await?;
        }

        Ok(())
    }
}
