use super::config::ProtocolConfig;
use crate::common::NodeId;
use crate::error::CommunicationError;
use crate::network::{NetworkCommandSender, NetworkEvent, NetworkEventReceiver};
use crypto::hash::Hash;
use crypto::signature::SignatureEngine;
use models::{Block, BlockHeader, SerializationContext};
use std::collections::{HashMap, HashSet};
use tokio::sync::mpsc;

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
    // Send a block to peers who asked for it.
    SendBlock {
        hash: Hash,
        block: Block,
    },
}

#[derive(Debug)]
pub enum ProtocolManagementCommand {}

/// Information about a node we are connected to,
/// essentially our view of its state.
///
/// Note: should we prune the set of known and wanted blocks during lifetime of a node connection?
/// Currently it would only be dropped alongside the rest when the node becomes inactive.
#[derive(Default)]
struct NodeInfo {
    /// The blocks the node "knows about",
    /// defined as the one the node propagated headers to us for.
    known_blocks: HashSet<Hash>,
    /// The blocks the node asked for.
    wanted_blocks: HashSet<Hash>,
}

pub struct ProtocolWorker {
    /// Protocol configuration.
    _cfg: ProtocolConfig,
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
            _cfg: cfg,
            network_command_sender,
            network_event_receiver,
            controller_event_tx,
            controller_command_rx,
            controller_manager_rx,
            active_nodes: HashMap::new(),
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
        loop {
            tokio::select! {
                // listen to incoming commands
                Some(cmd) = self.controller_command_rx.recv() => self.process_command(cmd).await?,

                // listen to network controller events
                evt = self.network_event_receiver.wait_event() => self.on_network_event(evt?).await?,

                // listen to management commands
                cmd = self.controller_manager_rx.recv() => match cmd {
                    None => break,
                    Some(_) => {}
                },
            } //end select!
        } //end loop

        Ok(self.network_event_receiver)
    }

    async fn process_command(&mut self, cmd: ProtocolCommand) -> Result<(), CommunicationError> {
        match cmd {
            ProtocolCommand::PropagateBlockHeader { hash, header } => {
                massa_trace!("block_header_propagation", { "block_header": header });
                for (node_id, node_info) in self.active_nodes.iter() {
                    if !node_info.known_blocks.contains(&hash) {
                        self.network_command_sender
                            .send_block_header(node_id.clone(), header.clone())
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
                // Ask for the block from all nodes who know it.
                // TODO: limit the number of nodes we ask the block from?
                for (node_id, node_info) in self.active_nodes.iter() {
                    if node_info.known_blocks.contains(&hash) {
                        self.network_command_sender
                            .ask_for_block(node_id.clone(), hash)
                            .await
                            .map_err(|_| {
                                CommunicationError::ChannelError(
                                    "ask for block network command send failed".into(),
                                )
                            })?;
                    }
                }
            }
            ProtocolCommand::SendBlock { hash, block } => {
                massa_trace!("send_block", { "block": block });
                // Send the block once to all nodes who asked for it.
                for (node_id, node_info) in self.active_nodes.iter_mut() {
                    if node_info.wanted_blocks.remove(&hash) {
                        node_info.known_blocks.insert(hash);
                        self.network_command_sender
                            .send_block(node_id.clone(), block.clone())
                            .await
                            .map_err(|_| {
                                CommunicationError::ChannelError(
                                    "send block node command send failed".into(),
                                )
                            })?;
                    }
                }
            }
        }
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
        node_info.known_blocks.insert(hash.clone());

        Ok(hash)
    }

    /// Manages network event
    /// Only used by the worker.
    ///
    /// # Argument
    /// evt: event to processs
    async fn on_network_event(&mut self, evt: NetworkEvent) -> Result<(), CommunicationError> {
        match evt {
            NetworkEvent::NewConnection(node_id) => {
                self.active_nodes.insert(node_id, Default::default());
            }
            NetworkEvent::ConnectionClosed(node_id) => {
                self.active_nodes.remove(&node_id);
            }
            NetworkEvent::ReceivedBlock(from_node_id, block) => {
                let hash = self.note_header_from_node(&block.header, &from_node_id)?;
                self.controller_event_tx
                    .send(ProtocolEvent::ReceivedBlock { hash, block })
                    .await
                    .map_err(|_| {
                        CommunicationError::ChannelError("receive block event send failed".into())
                    })
            }?,
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
                    })?
            }
        }
        Ok(())
    }
}
