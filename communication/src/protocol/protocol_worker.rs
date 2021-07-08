use super::config::ProtocolConfig;
use crate::common::NodeId;
use crate::error::CommunicationError;
use crate::network::{NetworkCommandSender, NetworkEvent, NetworkEventReceiver};
use crypto::hash::Hash;
use crypto::signature::SignatureEngine;
use models::{Block, BlockHeader, SerializationContext};
use std::collections::{HashMap, HashSet};
use time::TimeError;
use tokio::{
    sync::mpsc,
    time::{sleep, sleep_until, Instant, Sleep},
};

/// Possible types of events that can happen.
#[derive(Debug)]
pub enum ProtocolEvent {
    /// A isolated transaction was received.
    ReceivedTransaction(String),
    /// A block with a valid signature has been received.
    ReceivedBlock { hash: Hash, block: Block },
    /// A block header with a valid signature has been received.
    ReceivedBlockHeader { hash: Hash, header: BlockHeader },
    /// Ask for a block from consensus.
    GetBlock(Hash),
}

/// Commands that protocol worker can process
#[derive(Debug)]
pub enum ProtocolCommand {
    /// Propagate header of a given block.
    PropagateBlockHeader {
        hash: Hash,
        header: BlockHeader,
    },
    /// Propagate hash of a given block header
    AskForBlock(Hash),
    /// Send a block to peers who asked for it.
    SendBlock {
        hash: Hash,
        block: Block,
    },
    /// Wishlist delta
    WishlistDelta {
        new: HashSet<Hash>,
        remove: HashSet<Hash>,
    },
    BlockNotFound(Hash),
}

#[derive(Debug)]
pub enum ProtocolManagementCommand {}

/// Information about a node we are connected to,
/// essentially our view of its state.
///
/// Note: should we prune the set of known and wanted blocks during lifetime of a node connection?
/// Currently it would only be dropped alongside the rest when the node becomes inactive.
#[derive(Debug, Clone)]
struct NodeInfo {
    /// The blocks the node "knows about",
    /// defined as the one the node propagated headers to us for.
    known_blocks: HashMap<Hash, (bool, Instant)>,
    /// The blocks the node asked for.
    wanted_blocks: HashSet<Hash>,
    /// Blocks we asked that node for
    asked_blocks: HashMap<Hash, Instant>,
    /// Instant when the node was added
    connection_instant: Instant,
}

pub struct ProtocolWorker {
    /// Protocol configuration.
    cfg: ProtocolConfig,
    // Serialization context
    serialization_context: SerializationContext,
    /// Associated nework command sender.
    network_command_sender: NetworkCommandSender,
    /// Associated nework event receiver.
    network_event_receiver: NetworkEventReceiver,
    /// Channel to send protocol events to the controller.
    controller_event_tx: mpsc::Sender<ProtocolEvent>,
    /// Channel receiving commands from the controller.
    controller_command_rx: mpsc::Receiver<ProtocolCommand>,
    /// Channel to send management commands to the controller.
    controller_manager_rx: mpsc::Receiver<ProtocolManagementCommand>,
    /// Ids of active nodes mapped to node info.
    active_nodes: HashMap<NodeId, NodeInfo>,
    /// List of wanted blocks.
    block_wishlist: HashSet<Hash>,
}

impl ProtocolWorker {
    /// Creates a new protocol worker.
    ///
    /// # Arguments
    /// * cfg: protocol configuration.
    /// * self_node_id: our private key.
    /// * network_controller associated network controller.
    /// * controller_event_tx: Channel to send protocol events.
    /// * controller_command_rx: Channel receiving commands.
    /// * controller_manager_rx: Channel receiving management commands.
    pub fn new(
        cfg: ProtocolConfig,
        serialization_context: SerializationContext,
        network_command_sender: NetworkCommandSender,
        network_event_receiver: NetworkEventReceiver,
        controller_event_tx: mpsc::Sender<ProtocolEvent>,
        controller_command_rx: mpsc::Receiver<ProtocolCommand>,
        controller_manager_rx: mpsc::Receiver<ProtocolManagementCommand>,
    ) -> ProtocolWorker {
        ProtocolWorker {
            serialization_context,
            cfg,
            network_command_sender,
            network_event_receiver,
            controller_event_tx,
            controller_command_rx,
            controller_manager_rx,
            active_nodes: HashMap::new(),
            block_wishlist: HashSet::new(),
        }
    }

    /// Main protocol worker loop. Consumes self.
    /// It is mostly a tokio::select inside a loop
    /// wainting on :
    /// - controller_command_rx
    /// - network_controller
    /// - handshake_futures
    /// - node_event_rx
    /// And at the end every thing is closed properly
    /// Consensus work is managed here.
    /// It's mostly a tokio::select within a loop.
    pub async fn run_loop(mut self) -> Result<NetworkEventReceiver, CommunicationError> {
        let block_ask_timer = sleep(self.cfg.ask_block_timeout.into());
        tokio::pin!(block_ask_timer);
        loop {
            tokio::select! {
                // block ask timer
                _ = &mut block_ask_timer => self.update_ask_block(&mut block_ask_timer).await?,

                // listen to incoming commands
                Some(cmd) = self.controller_command_rx.recv() => self.process_command(cmd, &mut block_ask_timer).await?,

                // listen to network controller events
                evt = self.network_event_receiver.wait_event() => self.on_network_event(evt?, &mut block_ask_timer).await?,

                // listen to management commands
                cmd = self.controller_manager_rx.recv() => match cmd {
                    None => break,
                    Some(_) => {}
                },
            } //end select!
        } //end loop

        Ok(self.network_event_receiver)
    }

    async fn process_command(
        &mut self,
        cmd: ProtocolCommand,
        timer: &mut std::pin::Pin<&mut Sleep>,
    ) -> Result<(), CommunicationError> {
        match cmd {
            ProtocolCommand::PropagateBlockHeader { hash, header } => {
                massa_trace!("block_header_propagation", { "block_header": header });
                for (node_id, node_info) in self.active_nodes.iter() {
                    let cond = node_info.known_blocks.get(&hash);
                    // if we don't know if that node knowns that hash or if we know it doesn't
                    if cond.is_none() || (cond.is_some() && !cond.unwrap().0) {
                        self.network_command_sender
                            .send_block_header(*node_id, header.clone())
                            .await
                            .map_err(|_| {
                                CommunicationError::ChannelError(
                                    "send block header network command send failed".into(),
                                )
                            })?;
                    }
                }
            }
            ProtocolCommand::AskForBlock(hash) => {
                massa_trace!("ask_for_block", { "block": hash });
                // // Ask for the block from all nodes who know it.
                // // TODO: limit the number of nodes we ask the block from?

                // if None = self.block_ask_status.get(hash) {

                // } // if already asked or already received
                // for (node_id, node_info) in self.active_nodes.iter() {
                //     if node_info.known_blocks.get(&hash) == Some(&true) {
                //         self.network_command_sender
                //             .ask_for_block(*node_id, hash)
                //             .await
                //             .map_err(|_| {
                //                 CommunicationError::ChannelError(
                //                     "ask for block network command send failed".into(),
                //                 )
                //             })?;
                //     }
                // }
            }
            ProtocolCommand::SendBlock { hash, block } => {
                massa_trace!("send_block", { "block": block });
                // Send the block once to all nodes who asked for it.
                for (node_id, node_info) in self.active_nodes.iter_mut() {
                    if node_info.wanted_blocks.remove(&hash) {
                        node_info.known_blocks.insert(hash, (true, Instant::now()));
                        self.network_command_sender
                            .send_block(*node_id, block.clone())
                            .await
                            .map_err(|_| {
                                CommunicationError::ChannelError(
                                    "send block node command send failed".into(),
                                )
                            })?;
                    }
                }
            }
            ProtocolCommand::WishlistDelta { new, remove } => {
                self.stop_asking_blocks(remove)?;
                self.block_wishlist.extend(new);
                self.update_ask_block(timer).await?;
            }
            ProtocolCommand::BlockNotFound(hash) => {
                for (node_id, node_info) in self.active_nodes.iter() {
                    if node_info.wanted_blocks.contains(&hash) {
                        self.network_command_sender
                            .block_not_found(*node_id, hash)
                            .await?
                    }
                }
            }
        }
        Ok(())
    }

    fn stop_asking_blocks(
        &mut self,
        remove_hashes: HashSet<Hash>,
    ) -> Result<(), CommunicationError> {
        for node_info in self.active_nodes.values_mut() {
            node_info
                .asked_blocks
                .retain(|h, _| !remove_hashes.contains(h));
        }
        self.block_wishlist.retain(|h| !remove_hashes.contains(h));
        Ok(())
    }

    async fn update_ask_block(
        &mut self,
        ask_block_timer: &mut std::pin::Pin<&mut Sleep>,
    ) -> Result<(), CommunicationError> {
        let now = Instant::now();

        // init timer
        let mut next_tick = now
            .checked_add(self.cfg.ask_block_timeout.into())
            .ok_or(TimeError::TimeOverflowError)?;

        // list blocks to re-ask and from whom
        for hash in self.block_wishlist.iter() {
            let mut needs_ask = true;
            let mut best_candidate = None;

            for (node_id, node_info) in self.active_nodes.iter_mut() {
                let ask_time_opt = node_info.asked_blocks.get(hash);
                let (timeout_at_opt, timed_out) = if let Some(ask_time) = ask_time_opt {
                    let t = ask_time
                        .checked_add(self.cfg.ask_block_timeout.into())
                        .ok_or(TimeError::TimeOverflowError)?;
                    (Some(t), t <= now)
                } else {
                    (None, false)
                };
                let knows_block = node_info.known_blocks.get(&hash).clone();

                // check if the node recently told us it doesn't have the block
                if let Some((false, info_time)) = knows_block {
                    let info_expires = info_time
                        .checked_add(self.cfg.ask_block_timeout.into())
                        .ok_or(TimeError::TimeOverflowError)?;
                    if info_expires > now {
                        next_tick = std::cmp::min(next_tick, info_expires);
                        continue; // ignore candidate node
                    }
                }

                let candidate = match (timed_out, timeout_at_opt, knows_block) {
                    // not asked yet
                    (_, None, knowledge) => match knowledge {
                        Some((true, _)) => (0u8, None),
                        None => (1u8, None),
                        Some((false, _)) => (2u8, None),
                    },
                    // not timed out yet (note: recent DONTHAVBLOCK checked before the match)
                    (false, Some(timeout_at), _) => {
                        next_tick = std::cmp::min(next_tick, timeout_at);
                        needs_ask = false; // no need to reask
                        continue; // not a candidate
                    }
                    // timed out, supposed to have it
                    (true, Some(timeout_at), Some((true, info_time))) => {
                        if info_time < &timeout_at {
                            // info less recent than timeout: mark as not having it
                            node_info.known_blocks.insert(*hash, (false, timeout_at));
                            (2u8, ask_time_opt)
                        } else {
                            // told us it has it after a timeout: good candidate again
                            (0u8, ask_time_opt)
                        }
                    }
                    // timed out, supposed to not have it
                    (true, Some(timeout_at), Some((false, info_time))) => {
                        if info_time < &timeout_at {
                            // info less recent than timeout: update info time
                            node_info.known_blocks.insert(*hash, (false, timeout_at));
                        }
                        (2u8, ask_time_opt)
                    }
                    // timed out but don't know if has it: mark as not having it
                    (true, Some(timeout_at), None) => {
                        node_info.known_blocks.insert(*hash, (false, timeout_at));
                        (2u8, ask_time_opt)
                    }
                };

                // update candidate node
                if best_candidate.is_none()
                    || Some((candidate, node_info.connection_instant, *node_id)) < best_candidate
                {
                    best_candidate = Some((candidate, node_info.connection_instant, *node_id));
                }
            }

            // skip if doesn't need to be asked
            if !needs_ask {
                continue;
            }

            // ask the best node, if there is one and update timeout
            if let Some((_, _, node_id)) = best_candidate.take() {
                self.active_nodes
                    .get_mut(&node_id)
                    .unwrap()
                    .asked_blocks
                    .insert(*hash, now); // will not panic, already checked
                self.network_command_sender
                    .ask_for_block(node_id, *hash)
                    .await
                    .map_err(|_| {
                        CommunicationError::ChannelError(
                            "ask for block node command send failed".into(),
                        )
                    })?;
                let timeout_at = now
                    .checked_add(self.cfg.ask_block_timeout.into())
                    .ok_or(TimeError::TimeOverflowError)?;
                next_tick = std::cmp::min(next_tick, timeout_at);
            }
        }

        // reset timer
        ask_block_timer.set(sleep_until(next_tick));

        Ok(())
    }

    /// Check a header's signature, and if valid note the node knows the block.
    /// TODO: instead of propagating errors, which will break the run loop of the worker,
    /// handle them.
    fn note_header_from_node(
        &mut self,
        header: &BlockHeader,
        source_node_id: &NodeId,
    ) -> Result<Hash, CommunicationError> {
        let hash = header
            .content
            .compute_hash(&self.serialization_context)
            .map_err(|err| CommunicationError::HeaderHashError(err))?;

        // check signature
        if let Err(_err) = header.verify_signature(&hash, &SignatureEngine::new()) {
            return Err(CommunicationError::WrongSignature);
        }

        let node_info = self
            .active_nodes
            .get_mut(source_node_id)
            .ok_or(CommunicationError::MissingNodeError)?;
        node_info
            .known_blocks
            .insert(hash.clone(), (true, Instant::now()));
        // todo update wanted blocks

        Ok(hash)
    }

    /// Manages network event
    /// Only used by the worker.
    ///
    /// # Argument
    /// evt: event to processs
    async fn on_network_event(
        &mut self,
        evt: NetworkEvent,
        block_ask_timer: &mut std::pin::Pin<&mut Sleep>,
    ) -> Result<(), CommunicationError> {
        match evt {
            NetworkEvent::NewConnection(node_id) => {
                self.active_nodes.insert(
                    node_id,
                    NodeInfo {
                        known_blocks: HashMap::new(),
                        wanted_blocks: HashSet::new(),
                        asked_blocks: HashMap::new(),
                        connection_instant: Instant::now(),
                    },
                );
                self.update_ask_block(block_ask_timer).await?;
            }
            NetworkEvent::ConnectionClosed(node_id) => {
                self.active_nodes.remove(&node_id); // deletes all node info
                self.update_ask_block(block_ask_timer).await?;
            }
            NetworkEvent::ReceivedBlock(from_node_id, block) => {
                let hash = self.note_header_from_node(&block.header, &from_node_id)?;
                self.stop_asking_blocks(HashSet::from(vec![hash].into_iter().collect()))?;
                self.controller_event_tx
                    .send(ProtocolEvent::ReceivedBlock { hash, block })
                    .await
                    .map_err(|_| {
                        CommunicationError::ChannelError("receive block event send failed".into())
                    })?;
                self.update_ask_block(block_ask_timer).await?;
            }
            NetworkEvent::AskedForBlock(from_node_id, data) => {
                let node_info = self
                    .active_nodes
                    .get_mut(&from_node_id)
                    .ok_or(CommunicationError::MissingNodeError)?;
                node_info.wanted_blocks.insert(data.clone());
                self.controller_event_tx
                    .send(ProtocolEvent::GetBlock(data))
                    .await
                    .map_err(|_| {
                        CommunicationError::ChannelError(
                            "receive asked for block event send failed".into(),
                        )
                    })?;
            }
            NetworkEvent::ReceivedBlockHeader {
                source_node_id,
                header,
            } => {
                let hash = self.note_header_from_node(&header, &source_node_id)?;
                self.controller_event_tx
                    .send(ProtocolEvent::ReceivedBlockHeader { hash, header })
                    .await
                    .map_err(|_| {
                        CommunicationError::ChannelError("receive block event send failed".into())
                    })?;
                self.update_ask_block(block_ask_timer).await?;
            }
            NetworkEvent::BlockNotFound(node_id, hash) => {
                if let Some(info) = self.active_nodes.get_mut(&node_id) {
                    info.known_blocks.insert(hash, (false, Instant::now()));
                }
                self.update_ask_block(block_ask_timer).await?;
            }
        }
        Ok(())
    }
}
