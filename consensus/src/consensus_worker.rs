use crate::pool::OperationPool;

use super::{
    block_graph::*, config::ConsensusConfig, error::ConsensusError, random_selector::*,
    timeslots::*,
};
use communication::protocol::{ProtocolCommandSender, ProtocolEvent, ProtocolEventReceiver};
use crypto::{hash::Hash, signature::PublicKey, signature::SignatureEngine};
use models::{Block, Operation, SerializationContext, Slot};
use std::collections::{HashMap, HashSet};
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
        hash: Hash,
        response_tx: oneshot::Sender<Option<Block>>,
    },
    /// Returns through a channel the list of slots with public key of the selected staker.
    GetSelectionDraws {
        start: Slot,
        end: Slot,
        response_tx: oneshot::Sender<Result<Vec<(Slot, PublicKey)>, ConsensusError>>,
    },
    GetBootGraph(oneshot::Sender<BootsrapableGraph>),
    CreatedOperation(Operation),
}

/// Events that are emitted by consensus.
#[derive(Debug, Clone)]
pub enum ConsensusEvent {
    dummyevent,
}

/// Events that are emitted by consensus.
#[derive(Debug, Clone)]
pub enum ConsensusManagementCommand {
    dummycommand,
}

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
    wishlist: HashSet<Hash>,
    /// operation pool
    operation_pool: OperationPool,
    /// serialization_context
    context: SerializationContext,
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
        opt_storage_command_sender: Option<StorageAccess>,
        block_db: BlockGraph,
        controller_command_rx: mpsc::Receiver<ConsensusCommand>,
        controller_event_tx: mpsc::Sender<ConsensusEvent>,
        controller_manager_rx: mpsc::Receiver<ConsensusManagementCommand>,
        clock_compensation: i64,
        context: SerializationContext,
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

        massa_trace!("consensus.consensus_worker.new", {});
        Ok(ConsensusWorker {
            cfg: cfg.clone(),
            genesis_public_key: SignatureEngine::new().derive_public_key(&cfg.genesis_key),
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
            operation_pool: OperationPool::new(next_slot, cfg.clone()),
            context,
        })
    }

    /// Consensus work is managed here.
    /// It's mostly a tokio::select within a loop.
    pub async fn run_loop(mut self) -> Result<ProtocolEventReceiver, ConsensusError> {
        let next_slot_timer = sleep_until(
            get_block_slot_timestamp(
                self.cfg.thread_count,
                self.cfg.t0,
                self.cfg.genesis_timestamp,
                self.next_slot,
            )?
            .estimate_instant(0)?,
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
            let (hash, block) = self.block_db.create_block("block".to_string(), cur_slot)?;
            massa_trace!("consensus.consensus_worker.slot_tick.create_block", {"hash": hash, "block": block});
            self.block_db
                .incoming_block(hash, block, &mut self.selector, Some(cur_slot))?;
        }

        // signal tick to block graph
        self.block_db
            .slot_tick(&mut self.selector, Some(cur_slot))?;

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
            .estimate_instant(0)?,
        ));

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
            ConsensusCommand::GetActiveBlock { hash, response_tx } => {
                massa_trace!(
                    "consensus.consensus_worker.process_consensus_command.get_active_block",
                    {}
                );
                response_tx
                    .send(self.block_db.get_active_block(hash).cloned())
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
                    .send(BootsrapableGraph::from(&self.block_db))
                    .map_err(|err| {
                        ConsensusError::SendChannelError(format!(
                            "could not send GetBootGraph answer:{:?}",
                            err
                        ))
                    })
            }
            ConsensusCommand::CreatedOperation(operation) => {
                if let Ok(true) = self
                    .operation_pool
                    .new_operation(operation.clone(), &self.context)
                {
                    self.protocol_command_sender
                        .propagate_operation(operation)
                        .await
                        .map_err(|err| {
                            ConsensusError::SendChannelError(format!(
                                "could not send propagate operation command:{:?}",
                                err
                            ))
                        })
                } else {
                    Ok(())
                }
            }
        }
    }

    /// Manages received protocolevents.
    ///
    /// # Arguments
    /// * event: event type to process.
    async fn process_protocol_event(&mut self, event: ProtocolEvent) -> Result<(), ConsensusError> {
        match event {
            ProtocolEvent::ReceivedBlock { hash, block } => {
                massa_trace!("consensus.consensus_worker.process_protocol_event.received_block", { "hash": hash, "block": block });
                self.block_db.incoming_block(
                    hash,
                    block,
                    &mut self.selector,
                    self.previous_slot,
                )?;
                self.block_db_changed().await?;
            }
            ProtocolEvent::ReceivedBlockHeader { hash, header } => {
                massa_trace!("consensus.consensus_worker.process_protocol_event.received_header", { "hash": hash, "header": header });
                self.block_db.incoming_header(
                    hash,
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
                    if let Some(block) = self.block_db.get_active_block(block_hash) {
                        massa_trace!("consensus.consensus_worker.process_protocol_event.get_block.consensus_found", { "hash": block_hash});
                        results.insert(block_hash, Some(block.clone()));
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
            ProtocolEvent::ReceivedOperation(operation) => {
                if let Ok(true) = self
                    .operation_pool
                    .new_operation(operation.clone(), &self.context)
                {
                    self.protocol_command_sender
                        .propagate_operation(operation)
                        .await?;
                }
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

        Ok(())
    }
}
