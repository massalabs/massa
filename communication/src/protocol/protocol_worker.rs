use super::{
    common::NodeId,
    config::{ProtocolConfig, CHANNEL_SIZE},
    handshake_worker::{HandshakeReturnType, HandshakeWorker},
    node_worker::{NodeCommand, NodeEvent, NodeEventType, NodeWorker},
};
use crate::error::{CommunicationError, HandshakeErrorType};
use crate::network::{
    ConnectionClosureReason, ConnectionId, NetworkCommandSender, NetworkEvent, NetworkEventReceiver,
};
use crypto::{hash::Hash, signature::PrivateKey, signature::Signature};
use futures::{stream::FuturesUnordered, StreamExt};
use models::block::{Block, BlockHeader};
use std::collections::{hash_map, HashMap, HashSet};
use tokio::{sync::mpsc, task::JoinHandle};

/// Possible types of events that can happen.
#[derive(Debug)]
pub enum ProtocolEvent {
    /// A isolated transaction was received.
    ReceivedTransaction(String),
    /// A block was received
    ReceivedBlock(Block),
    /// A block header was received
    ReceivedBlockHeader {
        signature: Signature,
        header: BlockHeader,
    },
    /// Ask for a block from consensus.
    GetBlock(Hash),
}

/// Commands that protocol worker can process
#[derive(Debug)]
pub enum ProtocolCommand {
    /// Propagate header of a given block.
    PropagateBlockHeader {
        hash: Hash,
        signature: Signature,
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
struct NodeInfo {
    /// The matching connection id.
    connection_id: ConnectionId,
    /// The command sender for the node.
    node_command_tx: mpsc::Sender<NodeCommand>,
    /// The join handle for the node worker.
    join_handle: JoinHandle<Result<(), CommunicationError>>,
    /// The blocks the node "knows about",
    /// defined as the one the node propagated headers to us for.
    known_blocks: HashSet<Hash>,
    /// The blocks the node asked for.
    wanted_blocks: HashSet<Hash>,
}

impl NodeInfo {
    fn new(
        connection_id: ConnectionId,
        node_command_tx: mpsc::Sender<NodeCommand>,
        join_handle: JoinHandle<Result<(), CommunicationError>>,
    ) -> Self {
        NodeInfo {
            connection_id,
            node_command_tx,
            join_handle,
            known_blocks: Default::default(),
            wanted_blocks: Default::default(),
        }
    }
}

pub struct ProtocolWorker {
    /// Protocol configuration.
    cfg: ProtocolConfig,
    /// Our node id.
    self_node_id: NodeId,
    /// Our private key.
    private_key: PrivateKey,
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
    /// Set of connection id of node with running handshake.
    running_handshakes: HashSet<ConnectionId>,
    /// Running handshakes futures.
    handshake_futures: FuturesUnordered<JoinHandle<(ConnectionId, HandshakeReturnType)>>,
    /// Ids of active nodes mapped to node info.
    active_nodes: HashMap<NodeId, NodeInfo>,
    /// Channel for sending node events.
    node_event_tx: mpsc::Sender<NodeEvent>,
    /// Receiving channel for node events.
    node_event_rx: mpsc::Receiver<NodeEvent>,
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
        self_node_id: NodeId,
        private_key: PrivateKey,
        network_command_sender: NetworkCommandSender,
        network_event_receiver: NetworkEventReceiver,
        controller_event_tx: mpsc::Sender<ProtocolEvent>,
        controller_command_rx: mpsc::Receiver<ProtocolCommand>,
        controller_manager_rx: mpsc::Receiver<ProtocolManagementCommand>,
    ) -> ProtocolWorker {
        let (node_event_tx, node_event_rx) = mpsc::channel::<NodeEvent>(CHANNEL_SIZE);
        ProtocolWorker {
            cfg,
            self_node_id,
            private_key,
            network_command_sender,
            network_event_receiver,
            controller_event_tx,
            controller_command_rx,
            controller_manager_rx,
            running_handshakes: HashSet::new(),
            handshake_futures: FuturesUnordered::new(),
            active_nodes: HashMap::new(),
            node_event_tx,
            node_event_rx,
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

                // wait for a handshake future to complete
                Some(res) = self.handshake_futures.next() => {
                    let (conn_id, outcome) = res?;
                    self.on_handshake_finished(conn_id, outcome).await?;
                },

                // event received from a node
                evt = self.node_event_rx.recv() => self.on_node_event(
                    evt.ok_or(CommunicationError::ChannelError("node event rx failed".into()))?
                ).await?
            } //end select!
        } //end loop

        // Cleanup
        {
            let mut node_handle_set = FuturesUnordered::new();
            // drop sender
            drop(self.node_event_tx);
            // gather active node handles
            for (_, node_info) in self.active_nodes.drain() {
                node_handle_set.push(node_info.join_handle);
            }
            // drain incoming node events
            while let Some(_) = self.node_event_rx.recv().await {}
            // wait for node join handles
            while let Some(res) = node_handle_set.next().await {
                if let Err(err) = res {
                    return Err(CommunicationError::TokioTaskJoinError(err));
                }
            }
        }

        // wait for all running handshakes
        self.running_handshakes.clear();
        while let Some(_) = self.handshake_futures.next().await {}

        Ok(self.network_event_receiver)
    }

    async fn process_command(&mut self, cmd: ProtocolCommand) -> Result<(), CommunicationError> {
        match cmd {
            ProtocolCommand::PropagateBlockHeader {
                signature,
                hash,
                header,
            } => {
                massa_trace!("block_header_propagation", { "block_header": header });
                for node_info in self.active_nodes.values() {
                    if !node_info.known_blocks.contains(&hash) {
                        node_info
                            .node_command_tx
                            .send(NodeCommand::SendBlockHeader {
                                signature,
                                header: header.clone(),
                            })
                            .await
                            .map_err(|_| {
                                CommunicationError::ChannelError(
                                    "block header propagate node command send failed".into(),
                                )
                            })?;
                    }
                }
            }
            ProtocolCommand::AskForBlock(hash) => {
                massa_trace!("ask_for_block", { "block": hash });
                // Ask for the block from all nodes who know it.
                // TODO: limit the number of nodes we ask the block from?
                for node_info in self.active_nodes.values() {
                    if node_info.known_blocks.contains(&hash) {
                        node_info
                            .node_command_tx
                            .send(NodeCommand::AskForBlock(hash))
                            .await
                            .map_err(|_| {
                                CommunicationError::ChannelError(
                                    "ask for block node command send failed".into(),
                                )
                            })?;
                    }
                }
            }
            ProtocolCommand::SendBlock { hash, block } => {
                massa_trace!("send_block", { "block": block });
                // Send the block once to all nodes who asked for it.
                for node_info in self.active_nodes.values_mut() {
                    if node_info.wanted_blocks.remove(&hash) {
                        node_info
                            .node_command_tx
                            .send(NodeCommand::SendBlock(block.clone()))
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

    /// Manages network event
    /// Only used by the worker.
    ///
    /// # Argument
    /// evt: event to processs
    async fn on_network_event(&mut self, evt: NetworkEvent) -> Result<(), CommunicationError> {
        match evt {
            NetworkEvent::NewConnection((connection_id, reader, writer)) => {
                // add connection ID to running_handshakes
                // launch async handshake_fn(connectionId, socket)
                // add its handle to handshake_futures
                if !self.running_handshakes.insert(connection_id) {
                    return Err(CommunicationError::HandshakeError(
                        HandshakeErrorType::HandshakeIdAlreadyExistError(format!(
                            "{}",
                            connection_id
                        )),
                    ));
                }

                debug!("starting handshake with connection_id={:?}", connection_id);
                massa_trace!("handshake_start", { "connection_id": connection_id });

                let self_node_id = self.self_node_id;
                let private_key = self.private_key;
                let message_timeout = self.cfg.message_timeout;
                let connection_id_copy = connection_id.clone();
                let handshake_fn_handle = tokio::spawn(async move {
                    (
                        connection_id_copy,
                        HandshakeWorker::new(
                            reader,
                            writer,
                            self_node_id,
                            private_key,
                            message_timeout,
                        )
                        .run()
                        .await,
                    )
                });
                self.handshake_futures.push(handshake_fn_handle);
            }
            NetworkEvent::ConnectionBanned(connection_id) => {
                // connection_banned(connectionId)
                // remove the connectionId entry in running_handshakes
                self.running_handshakes.remove(&connection_id);
                // find all active_node with this ConnectionId and send a NodeMessage::Close
                for node_info in self.active_nodes.values() {
                    if node_info.connection_id == connection_id {
                        node_info
                            .node_command_tx
                            .send(NodeCommand::Close)
                            .await
                            .map_err(|_| {
                                CommunicationError::ChannelError(
                                    "node close command send failed".into(),
                                )
                            })?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Manages finished handshakes.
    /// Only used by the worker.
    ///
    /// # Arguments
    /// * new_connection_id: connection id of the connection that should be established here.
    /// * outcome: result returned by a handshake.
    async fn on_handshake_finished(
        &mut self,
        new_connection_id: ConnectionId,
        outcome: HandshakeReturnType,
    ) -> Result<(), CommunicationError> {
        match outcome {
            // a handshake finished, and succeeded
            Ok((new_node_id, socket_reader, socket_writer)) => {
                debug!(
                    "handshake with connection_id={:?} succeeded => node_id={:?}",
                    new_connection_id, new_node_id
                );
                massa_trace!("handshake_ok", {
                    "connection_id": new_connection_id,
                    "node_id": new_node_id
                });

                // connection was banned in the meantime
                if !self.running_handshakes.remove(&new_connection_id) {
                    debug!(
                        "connection_id={:?}, node_id={:?} peer was banned while handshaking",
                        new_connection_id, new_node_id
                    );
                    massa_trace!("handshake_banned", {
                        "connection_id": new_connection_id,
                        "node_id": new_node_id
                    });
                    self.network_command_sender
                        .connection_closed(new_connection_id, ConnectionClosureReason::Normal)
                        .await?;
                    return Ok(());
                }

                match self.active_nodes.entry(new_node_id) {
                    // we already have this node ID
                    hash_map::Entry::Occupied(_) => {
                        debug!(
                            "connection_id={:?}, node_id={:?} protocol channel would be redundant",
                            new_connection_id, new_node_id
                        );
                        massa_trace!("node_redundant", {
                            "connection_id": new_connection_id,
                            "node_id": new_node_id
                        });
                        self.network_command_sender
                            .connection_closed(new_connection_id, ConnectionClosureReason::Normal)
                            .await?;
                    }
                    // we don't have this node ID
                    hash_map::Entry::Vacant(entry) => {
                        info!(
                            "established protocol channel with connection_id={:?} => node_id={:?}",
                            new_connection_id, new_node_id
                        );
                        massa_trace!("node_connected", {
                            "connection_id": new_connection_id,
                            "node_id": new_node_id
                        });
                        // notidy that the connection is alive
                        self.network_command_sender
                            .connection_alive(new_connection_id)
                            .await?;
                        // spawn node_controller_fn
                        let (node_command_tx, node_command_rx) =
                            mpsc::channel::<NodeCommand>(CHANNEL_SIZE);
                        let node_event_tx_clone = self.node_event_tx.clone();
                        let cfg_copy = self.cfg.clone();
                        let node_fn_handle = tokio::spawn(async move {
                            NodeWorker::new(
                                cfg_copy,
                                new_node_id,
                                socket_reader,
                                socket_writer,
                                node_command_rx,
                                node_event_tx_clone,
                            )
                            .run_loop()
                            .await
                        });
                        entry.insert(NodeInfo::new(
                            new_connection_id,
                            node_command_tx,
                            node_fn_handle,
                        ));
                    }
                }
            }
            // a handshake finished and failed
            Err(err) => {
                debug!(
                    "handshake failed with connection_id={:?}: {:?}",
                    new_connection_id, err
                );
                massa_trace!("handshake_failed", {
                    "connection_id": new_connection_id,
                    "err": err.to_string()
                });
                self.running_handshakes.remove(&new_connection_id);
                self.network_command_sender
                    .connection_closed(new_connection_id, ConnectionClosureReason::Failed)
                    .await?;
            }
        };
        Ok(())
    }

    /// Manages node events.
    /// Only used by the worker.
    ///
    /// # Argument
    /// * evt: optional node event to process.
    async fn on_node_event(&mut self, evt: NodeEvent) -> Result<(), CommunicationError> {
        match evt {
            // received a list of peers
            NodeEvent(from_node_id, NodeEventType::ReceivedPeerList(lst)) => {
                debug!("node_id={:?} sent us a peer list: {:?}", from_node_id, lst);
                massa_trace!("peer_list_received", {
                    "node_id": from_node_id,
                    "ips": lst
                });
                let node_info = self
                    .active_nodes
                    .get(&from_node_id)
                    .ok_or(CommunicationError::MissingNodeError)?;
                self.network_command_sender
                    .connection_alive(node_info.connection_id)
                    .await?;
                self.network_command_sender
                    .merge_advertised_peer_list(lst)
                    .await?;
            }
            NodeEvent(_from_node_id, NodeEventType::ReceivedBlock(data)) => self
                .controller_event_tx
                .send(ProtocolEvent::ReceivedBlock(data))
                .await
                .map_err(|_| {
                    CommunicationError::ChannelError("receive block event send failed".into())
                })?,
            NodeEvent(from_node_id, NodeEventType::ReceivedAskForBlock(data)) => {
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
                    })?
            }
            NodeEvent(from_node_id, NodeEventType::ReceivedBlockHeader { signature, header }) => {
                let node_info = self
                    .active_nodes
                    .get_mut(&from_node_id)
                    .ok_or(CommunicationError::MissingNodeError)?;
                node_info.known_blocks.insert(
                    header
                        .compute_hash()
                        .map_err(|err| CommunicationError::HeaderHashError(err))?,
                );
                self.controller_event_tx
                    .send(ProtocolEvent::ReceivedBlockHeader { signature, header })
                    .await
                    .map_err(|_| {
                        CommunicationError::ChannelError("receive block event send failed".into())
                    })?
            }
            NodeEvent(_from_node_id, NodeEventType::ReceivedTransaction(data)) => self
                .controller_event_tx
                .send(ProtocolEvent::ReceivedTransaction(data))
                .await
                .map_err(|_| {
                    CommunicationError::ChannelError("receive transaction event send failed".into())
                })?,
            // connection closed
            NodeEvent(from_node_id, NodeEventType::Closed(reason)) => {
                let node_info = self
                    .active_nodes
                    .remove(&from_node_id)
                    .ok_or(CommunicationError::MissingNodeError)?;
                info!("protocol channel closed peer_id={:?}", from_node_id);
                massa_trace!("node_closed", {
                    "node_id": from_node_id,
                    "reason": reason
                });
                self.network_command_sender
                    .connection_closed(node_info.connection_id, reason)
                    .await?;
                node_info.join_handle.await??;
            }
            // asked peer list
            NodeEvent(from_node_id, NodeEventType::AskedPeerList) => {
                debug!("node_id={:?} asked us for peer list", from_node_id);
                massa_trace!("node_asked_peer_list", { "node_id": from_node_id });
                let node_info = self
                    .active_nodes
                    .get(&from_node_id)
                    .ok_or(CommunicationError::MissingNodeError)?;
                node_info
                    .node_command_tx
                    .send(NodeCommand::SendPeerList(
                        self.network_command_sender
                            .get_advertisable_peer_list()
                            .await?,
                    ))
                    .await
                    .map_err(|_| {
                        CommunicationError::ChannelError(
                            "node command send send_peer_list failed".into(),
                        )
                    })?
            }
        }
        Ok(())
    }
}
