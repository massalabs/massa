use super::{
    block_graph::*,
    config::ConsensusConfig,
    error::{BlockAcknowledgeError, ConsensusError},
    misc_collections::{DependencyWaitingBlocks, FutureIncomingBlocks},
    random_selector::*,
    timeslots::*,
};
use communication::protocol::{ProtocolCommandSender, ProtocolEvent, ProtocolEventReceiver};
use crypto::{hash::Hash, signature::PublicKey, signature::SignatureEngine};
use models::{Block, Slot};
use std::collections::HashMap;
use storage::StorageCommandSender;
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
    GetActiveBlock(Hash, oneshot::Sender<Option<Block>>),
    /// Returns through a channel the list of slots with public key of the selected staker.
    GetSelectionDraws(
        Slot,
        Slot,
        oneshot::Sender<Result<Vec<(Slot, PublicKey)>, ConsensusError>>,
    ),
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
    /// Associated storage command sender. If we want to have long term final blocks storage.
    opt_storage_command_sender: Option<StorageCommandSender>,
    /// Database containing all information about blocks, the blockgraph and cliques.
    block_db: BlockGraph,
    /// Channel receiving consensus commands.
    controller_command_rx: mpsc::Receiver<ConsensusCommand>,
    /// Channel sending out consensus events.
    controller_event_tx: mpsc::Sender<ConsensusEvent>,
    /// Channel receiving consensus management commands.
    controller_manager_rx: mpsc::Receiver<ConsensusManagementCommand>,
    /// Selector used to select roll numbers.
    selector: RandomSelector,
    /// Sophisticated queue of blocks with slots in the near future.
    future_incoming_blocks: FutureIncomingBlocks,
    /// Sophisticated queue of blocks wainting for dependencies.
    dependency_waiting_blocks: DependencyWaitingBlocks,
    /// Current slot.
    current_slot: Slot,
}

/// Returned by acknowledge_block
#[derive(Default, Debug)]
struct AcknowledgeBlockReturn {
    /// Blocks that were depending on freshly acknowledged block, that we can try to acknowledge again
    pub to_retry: HashMap<crypto::hash::Hash, Block>,
    /// Final blocks that we may send to storage
    pub finals: HashMap<crypto::hash::Hash, Block>,
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
        opt_storage_command_sender: Option<StorageCommandSender>,
        block_db: BlockGraph,
        controller_command_rx: mpsc::Receiver<ConsensusCommand>,
        controller_event_tx: mpsc::Sender<ConsensusEvent>,
        controller_manager_rx: mpsc::Receiver<ConsensusManagementCommand>,
    ) -> Result<ConsensusWorker, ConsensusError> {
        let seed = vec![0u8; 32]; // TODO temporary (see issue #103)
        let participants_weights = vec![1u64; cfg.nodes.len()]; // TODO (see issue #104)
        let selector = RandomSelector::new(&seed, cfg.thread_count, participants_weights)?;
        let current_slot =
            get_current_latest_block_slot(cfg.thread_count, cfg.t0, cfg.genesis_timestamp)?
                .map_or(Ok(Slot::new(0u64, 0u8)), |s| {
                    s.get_next_slot(cfg.thread_count)
                })?;
        Ok(ConsensusWorker {
            cfg: cfg.clone(),
            genesis_public_key: SignatureEngine::new().derive_public_key(&cfg.genesis_key),
            protocol_command_sender,
            protocol_event_receiver,
            opt_storage_command_sender,
            block_db,
            controller_command_rx,
            controller_event_tx,
            controller_manager_rx,
            selector,
            future_incoming_blocks: FutureIncomingBlocks::new(cfg.max_future_processing_blocks),
            dependency_waiting_blocks: DependencyWaitingBlocks::new(cfg.max_dependency_blocks),
            current_slot,
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
                self.current_slot,
            )?
            .estimate_instant()?,
        );
        tokio::pin!(next_slot_timer);

        enum ConsensusWorkerEvent {
            Slot,
            Command(ConsensusCommand),
            FromProtocol(ProtocolEvent),
            Management(ConsensusManagementCommand),
        }

        loop {
            let event = tokio::select! {
                // slot timer
                _ = &mut next_slot_timer => ConsensusWorkerEvent::Slot,

                // listen consensus commands
                Some(cmd) = self.controller_command_rx.recv() => ConsensusWorkerEvent::Command(cmd),

                // receive protocol controller events
                evt = self.protocol_event_receiver.wait_event() => match evt {
                    Ok(event) => ConsensusWorkerEvent::FromProtocol(event),
                    Err(err) => return Err(ConsensusError::CommunicationError(err))
                },

                // listen to manager commands
                cmd = self.controller_manager_rx.recv() => match cmd {
                    None => break,
                    Some(cmd) => ConsensusWorkerEvent::Management(cmd)
                },
            };

            // 1. Handle new event
            let current_slot = match event {
                ConsensusWorkerEvent::Slot => self.slot_tick(&mut next_slot_timer).await?,
                ConsensusWorkerEvent::Command(cmd) => {
                    self.process_consensus_command(cmd).await?;
                    self.current_slot
                }
                ConsensusWorkerEvent::FromProtocol(event) => {
                    self.process_protocol_event(event).await?;
                    self.current_slot
                }
                ConsensusWorkerEvent::Management(cmd) => self.current_slot,
            };

            // 2. Run an ack checkpoint.
            let finals = self.block_db.run_ack_checkpoint(&current_slot);
            if let Some(cmd_tx) = &self.opt_storage_command_sender {
                cmd_tx.add_block_batch(finals).await?
            }

            // 3. Run a pruning checkpoint(prunes queues and discarded, active are pruned at 2).
            self.block_db.run_pruning_checkpoint(&current_slot);
        }
        // end loop
        Ok(self.protocol_event_receiver)
    }

    async fn slot_tick(
        &mut self,
        next_slot_timer: &mut std::pin::Pin<&mut Sleep>,
    ) -> Result<Slot, ConsensusError> {
        massa_trace!("slot_timer", {
            "slot": self.current_slot,
        });
        let block_creator = self.selector.draw(self.current_slot);

        // create a block if enabled and possible
        if !self.cfg.disable_block_creation
            && self.current_slot.period > 0
            && block_creator == self.cfg.current_node_index
        {
            self.block_db
                .create_block("block".to_string(), self.current_slot)?;
        }

        // reset timer for next slot
        let current_slot = self.current_slot;
        self.current_slot = self.current_slot.get_next_slot(self.cfg.thread_count)?;
        next_slot_timer.set(sleep_until(
            get_block_slot_timestamp(
                self.cfg.thread_count,
                self.cfg.t0,
                self.cfg.genesis_timestamp,
                self.current_slot,
            )?
            .estimate_instant()?,
        ));

        Ok(current_slot)
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
            ConsensusCommand::GetBlockGraphStatus(response_tx) => response_tx
                .send(BlockGraphExport::from(&self.block_db))
                .map_err(|err| {
                    ConsensusError::SendChannelError(format!(
                        "could not send GetBlockGraphStatus answer:{:?}",
                        err
                    ))
                }),
            //return full block with specified hash
            ConsensusCommand::GetActiveBlock(hash, response_tx) => response_tx
                .send(self.block_db.get_active_block(hash).cloned())
                .map_err(|err| {
                    ConsensusError::SendChannelError(format!(
                        "could not send GetBlock answer:{:?}",
                        err
                    ))
                }),
            ConsensusCommand::GetSelectionDraws(slot_start, slot_end, response_tx) => {
                let mut result: Result<Vec<(Slot, PublicKey)>, ConsensusError> = Ok(Vec::new());
                let mut cur_slot = slot_start;
                while cur_slot < slot_end {
                    if let Ok(res) = result.as_mut() {
                        res.push((
                            cur_slot,
                            if cur_slot.period == 0 {
                                self.genesis_public_key
                            } else {
                                self.cfg.nodes[self.selector.draw(cur_slot) as usize].0
                            },
                        ));
                    }
                    cur_slot = match cur_slot.get_next_slot(self.cfg.thread_count) {
                        Ok(next_slot) => next_slot,
                        Err(_) => {
                            result = Err(ConsensusError::SlotOverflowError);
                            break;
                        }
                    }
                }
                response_tx.send(result).map_err(|err| {
                    ConsensusError::SendChannelError(format!(
                        "could not send GetSelectionDraws response: {:?}",
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
            ProtocolEvent::ReceivedBlock { hash, block } => {
                self.block_db.note_block(hash, block, self.current_slot);
            }
            ProtocolEvent::ReceivedBlockHeader { hash, header } => {
                let header_check = self.block_db.note_header(
                    &hash,
                    &header,
                    &mut self.selector,
                    self.current_slot,
                );
                if header_check.is_ok() {
                    self.protocol_command_sender.ask_for_block(hash).await?;
                }
            }
            ProtocolEvent::ReceivedTransaction(_transaction) => {
                // todo (see issue #108)
            }
            ProtocolEvent::GetBlock(block_hash) => {
                if let Some(block) = self.block_db.get_active_block(block_hash) {
                    self.protocol_command_sender
                        .send_block(block_hash, block.clone())
                        .await?;
                } else if let Some(storage_command_sender) = &self.opt_storage_command_sender {
                    if let Some(block) = storage_command_sender.get_block(block_hash).await? {
                        self.protocol_command_sender
                            .send_block(block_hash, block)
                            .await?;
                    }
                }
            }
        }
        Ok(())
    }
}
