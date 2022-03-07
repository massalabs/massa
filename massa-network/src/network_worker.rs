// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! The network worker actually does the job of managing connections
use super::{
    common::{ConnectionClosureReason, ConnectionId},
    establisher::{Establisher, Listener, ReadHalf, WriteHalf},
    handshake_worker::HandshakeReturnType,
    node_worker::{NodeCommand, NodeEvent, NodeEventType, NodeWorker},
    peer_info_database::*,
};
use crate::{
    binders::{ReadBinder, WriteBinder},
    error::{HandshakeErrorType, NetworkConnectionErrorType, NetworkError},
    handshake_worker::HandshakeWorker,
    messages::Message,
    settings::NetworkSettings,
};
use futures::{stream::FuturesUnordered, StreamExt};
use massa_hash::hash::Hash;
use massa_logging::massa_trace;
use massa_models::{
    composite::PubkeySig, constants::CHANNEL_SIZE, node::NodeId, signed::Signed,
    stats::NetworkStats, with_serialization_context, DeserializeCompact, DeserializeVarInt,
    Endorsement, EndorsementId, ModelsError, Operation, OperationId, SerializeCompact,
    SerializeVarInt, Version,
};
use massa_models::{signed::Signable, BlockHeader};
use massa_models::{Block, BlockId};
use massa_signature::{derive_public_key, sign, PrivateKey};
use serde::{Deserialize, Serialize};
use std::{
    collections::{hash_map, HashMap, HashSet},
    convert::TryInto,
    net::{IpAddr, SocketAddr},
};
use tokio::sync::mpsc::error::SendTimeoutError;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::{debug, trace, warn};

/// Commands that the worker can execute
#[derive(Debug)]
pub enum NetworkCommand {
    /// Ask for a block from a node.
    AskForBlocks {
        list: HashMap<NodeId, Vec<BlockId>>,
    },
    /// Send that block to node.
    SendBlock {
        node: NodeId,
        block: Block,
    },
    /// Send a header to a node.
    SendBlockHeader {
        node: NodeId,
        header: Signed<BlockHeader, BlockId>,
    },
    // (PeerInfo, Vec <(NodeId, bool)>) peer info + list of associated Id nodes in connexion out (true)
    GetPeers(oneshot::Sender<Peers>),
    GetBootstrapPeers(oneshot::Sender<BootstrapPeers>),
    Ban(NodeId),
    BanIp(Vec<IpAddr>),
    Unban(Vec<IpAddr>),
    BlockNotFound {
        node: NodeId,
        block_id: BlockId,
    },
    SendOperations {
        node: NodeId,
        operations: Vec<Signed<Operation, OperationId>>,
    },
    SendEndorsements {
        node: NodeId,
        endorsements: Vec<Signed<Endorsement, EndorsementId>>,
    },
    NodeSignMessage {
        msg: Vec<u8>,
        response_tx: oneshot::Sender<PubkeySig>,
    },
    GetStats {
        response_tx: oneshot::Sender<NetworkStats>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    pub peer_info: PeerInfo,
    pub active_nodes: Vec<(NodeId, bool)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peers {
    pub our_node_id: NodeId,
    pub peers: HashMap<IpAddr, Peer>,
}

#[derive(Debug)]
pub enum NetworkEvent {
    NewConnection(NodeId),
    ConnectionClosed(NodeId),
    /// A block was received
    ReceivedBlock {
        node: NodeId,
        block: Block,
    },
    /// A block header was received
    ReceivedBlockHeader {
        source_node_id: NodeId,
        header: Signed<BlockHeader, BlockId>,
    },
    /// Someone ask for block with given header hash.
    AskedForBlocks {
        node: NodeId,
        list: Vec<BlockId>,
    },
    /// That node does not have this block
    BlockNotFound {
        node: NodeId,
        block_id: BlockId,
    },
    ReceivedOperations {
        node: NodeId,
        operations: Vec<Signed<Operation, OperationId>>,
    },
    ReceivedEndorsements {
        node: NodeId,
        endorsements: Vec<Signed<Endorsement, EndorsementId>>,
    },
}

#[derive(Debug)]
pub enum NetworkManagementCommand {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapPeers(pub Vec<IpAddr>);

impl SerializeCompact for BootstrapPeers {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, massa_models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // peers
        let peers_count: u32 = self.0.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!("too many peers blocks in BootstrapPeers: {}", err))
        })?;
        let max_peer_list_length =
            with_serialization_context(|context| context.max_advertise_length);
        if peers_count > max_peer_list_length {
            return Err(ModelsError::SerializeError(format!(
                "too many peers for serialization context in BootstrapPeers: {}",
                peers_count
            )));
        }
        res.extend(peers_count.to_varint_bytes());
        for peer in self.0.iter() {
            res.extend(peer.to_bytes_compact()?);
        }

        Ok(res)
    }
}

impl DeserializeCompact for BootstrapPeers {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), massa_models::ModelsError> {
        let mut cursor = 0usize;

        // peers
        let (peers_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        let max_peer_list_length =
            with_serialization_context(|context| context.max_advertise_length);
        if peers_count > max_peer_list_length {
            return Err(ModelsError::DeserializeError(format!(
                "too many peers for deserialization context in BootstrapPeers: {}",
                peers_count
            )));
        }
        cursor += delta;
        let mut peers: Vec<IpAddr> = Vec::with_capacity(peers_count as usize);
        for _ in 0..(peers_count as usize) {
            let (ip, delta) = IpAddr::from_bytes_compact(&buffer[cursor..])?;
            cursor += delta;
            peers.push(ip);
        }

        Ok((BootstrapPeers(peers), cursor))
    }
}

/// Real job is done by network worker
pub struct NetworkWorker {
    /// Network configuration.
    cfg: NetworkSettings,
    /// Our private key.
    private_key: PrivateKey,
    /// Our node id.
    self_node_id: NodeId,
    /// Listener part of the establisher.
    listener: Listener,
    /// The connection establisher.
    establisher: Establisher,
    /// Database with peer information.
    peer_info_db: PeerInfoDatabase,
    /// Receiver for network commands
    controller_command_rx: mpsc::Receiver<NetworkCommand>,
    /// Sender for network events
    controller_event_tx: mpsc::Sender<NetworkEvent>,
    /// Receiver for network management commands
    controller_manager_rx: mpsc::Receiver<NetworkManagementCommand>,
    /// Set of connection id of node with running handshake.
    running_handshakes: HashSet<ConnectionId>,
    /// Running handshakes futures.
    handshake_futures: FuturesUnordered<JoinHandle<(ConnectionId, HandshakeReturnType)>>,
    /// Running handshakes that send a list of peers.
    handshake_peer_list_futures: FuturesUnordered<JoinHandle<()>>,
    /// Channel for sending node events.
    node_event_tx: mpsc::Sender<NodeEvent>,
    /// Receiving channel for node events.
    node_event_rx: mpsc::Receiver<NodeEvent>,
    /// Ids of active nodes mapped to Connection id, node command sender and handle on the associated node worker.
    active_nodes: HashMap<NodeId, (ConnectionId, mpsc::Sender<NodeCommand>)>,
    /// Node worker handles
    node_worker_handles:
        FuturesUnordered<JoinHandle<(NodeId, Result<ConnectionClosureReason, NetworkError>)>>,
    /// Map of connection to ip, is_outgoing.
    active_connections: HashMap<ConnectionId, (IpAddr, bool)>,
    version: Version,
}

pub struct NetworkWorkerChannels {
    pub controller_command_rx: mpsc::Receiver<NetworkCommand>,
    pub controller_event_tx: mpsc::Sender<NetworkEvent>,
    pub controller_manager_rx: mpsc::Receiver<NetworkManagementCommand>,
}

impl NetworkWorker {
    /// Creates a new NetworkWorker
    ///
    /// # Arguments
    /// * cfg: Network configuration.
    /// * listener: Listener part of the establisher.
    /// * establisher: The connection establisher.
    /// * peer_info_db: Database with peer information.
    /// * controller_command_rx: Channel receiving network commands.
    /// * controller_event_tx: Channel sending out network events.
    /// * controller_manager_rx: Channel receiving network management commands.
    pub fn new(
        cfg: NetworkSettings,
        private_key: PrivateKey,
        listener: Listener,
        establisher: Establisher,
        peer_info_db: PeerInfoDatabase,
        NetworkWorkerChannels {
            controller_command_rx,
            controller_event_tx,
            controller_manager_rx,
        }: NetworkWorkerChannels,
        version: Version,
    ) -> NetworkWorker {
        let public_key = derive_public_key(&private_key);
        let self_node_id = NodeId(public_key);

        let (node_event_tx, node_event_rx) = mpsc::channel::<NodeEvent>(CHANNEL_SIZE);
        NetworkWorker {
            cfg,
            self_node_id,
            private_key,
            listener,
            establisher,
            peer_info_db,
            controller_command_rx,
            controller_event_tx,
            controller_manager_rx,
            running_handshakes: HashSet::new(),
            handshake_futures: FuturesUnordered::new(),
            handshake_peer_list_futures: FuturesUnordered::new(),
            node_event_tx,
            node_event_rx,
            active_nodes: HashMap::new(),
            node_worker_handles: FuturesUnordered::new(),
            active_connections: HashMap::new(),
            version,
        }
    }

    async fn send_network_event(&self, event: NetworkEvent) -> Result<(), NetworkError> {
        let result = self
            .controller_event_tx
            .send_timeout(event, self.cfg.max_send_wait.to_duration())
            .await;
        match result {
            Ok(()) => return Ok(()),
            Err(SendTimeoutError::Closed(event)) => {
                debug!(
                    "Failed to send NetworkEvent due to channel closure: {:?}.",
                    event
                );
            }
            Err(SendTimeoutError::Timeout(event)) => {
                debug!("Failed to send NetworkEvent due to timeout: {:?}.", event);
            }
        }
        Err(NetworkError::ChannelError("Failed to send event.".into()))
    }

    /// Runs the main loop of the network_worker
    /// There is a tokio::select! inside the loop
    pub async fn run_loop(mut self) -> Result<(), NetworkError> {
        let mut out_connecting_futures = FuturesUnordered::new();
        let mut cur_connection_id = ConnectionId::default();

        // wake up the controller at a regular interval to retry connections
        let mut wakeup_interval = tokio::time::interval(self.cfg.wakeup_interval.to_duration());
        let mut need_connect_retry = true;

        loop {
            if need_connect_retry {
                // try to connect to candidate IPs
                let candidate_ips = self.peer_info_db.get_out_connection_candidate_ips()?;
                for ip in candidate_ips {
                    debug!("starting outgoing connection attempt towards ip={}", ip);
                    massa_trace!("out_connection_attempt_start", { "ip": ip });
                    self.peer_info_db.new_out_connection_attempt(&ip)?;
                    let mut connector = self
                        .establisher
                        .get_connector(self.cfg.connect_timeout)
                        .await?;
                    let addr = SocketAddr::new(ip, self.cfg.protocol_port);
                    out_connecting_futures.push(async move {
                        match connector.connect(addr).await {
                            Ok((reader, writer)) => (addr.ip(), Ok((reader, writer))),
                            Err(e) => (addr.ip(), Err(e)),
                        }
                    });
                }
                need_connect_retry = false;
            }

            /*
                select! without the "biased" modifier will randomly select the 1st branch to check,
                then will check the next ones in the order they are written.
                We choose this order:
                    * manager commands to avoid waiting too long to stop in case of contention
                    * node events (HIGH FREQUENCY): we want to process incoming events in priority to know best about the network and empty buffers quickly
                    * incoming commands (HIGH FREQUENCY): we want to TRY to send data to the target, but don't wait if the buffers are full
                    * cleanup tick (less important, low freq)
                    * node closed (no worries if processed a bit late)
                    * out connecting events (no problem if a bit late)
                    * listener event (HIGH FREQUENCY) non-critical
            */
            tokio::select! {
                // listen to manager commands
                cmd = self.controller_manager_rx.recv() => {
                    match cmd {
                        None => break,
                        Some(_) => {}
                    }
                },

                // event received from a node
                evt = self.node_event_rx.recv() => {
                    self.on_node_event(
                        evt.ok_or_else(|| NetworkError::ChannelError("node event rx failed".into()))?
                    ).await?
                },

                // incoming command
                Some(cmd) = self.controller_command_rx.recv() => {
                    self.manage_network_command(cmd).await?;
                },

                // wake up interval
                _ = wakeup_interval.tick() => {
                    self.peer_info_db.update()?; // notify tick to peer db

                    need_connect_retry = true; // retry out connections
                }

                // wait for a handshake future to complete
                Some(res) = self.handshake_futures.next() => {
                    let (conn_id, outcome) = res?;
                    self.on_handshake_finished(conn_id, outcome).await?;
                    need_connect_retry = true; // retry out connections
                },

                // Managing handshakes that return a PeerList
                Some(_) = self.handshake_peer_list_futures.next() => {},

                // node closed
                Some(evt) = self.node_worker_handles.next() => {
                    let (node_id, res) = evt?;  // ? => when a node worker panics
                    let reason = match res {
                        Ok(r) => {
                            massa_trace!("network.network_worker.run_loop.node_worker_handles.normal", {
                                "node_id": node_id,
                                "reason": r,
                            });
                            r
                        },
                        Err(err) => {
                            massa_trace!("network.network_worker.run_loop.node_worker_handles.err", {
                                "node_id": node_id,
                                "err": format!("{}", err)
                            });
                            ConnectionClosureReason::Failed
                        }
                    };

                    // Note: if the send is dropped, and we later receive a command related to an unknown node,
                    // we will retry a send for this event for that unknown node,
                    // ensuring protocol eventually notes the closure.
                    let _ = self
                        .send_network_event(NetworkEvent::ConnectionClosed(node_id))
                        .await;
                    if let Some((connection_id, _)) = self
                        .active_nodes
                        .remove(&node_id) {
                        massa_trace!("protocol channel closed", {"node_id": node_id});
                        self.connection_closed(connection_id, reason).await?;
                    }

                    need_connect_retry = true; // retry out connections
                },

                // out-connector event
                Some((ip_addr, res)) = out_connecting_futures.next() => {
                    need_connect_retry = true; // retry out connections
                    self.manage_out_connections(
                        res,
                        ip_addr,
                        &mut cur_connection_id,
                    ).await?
                },

                // listener socket received
                res = self.listener.accept() => {
                    self.manage_in_connections(
                        res,
                        &mut cur_connection_id,
                    ).await?
                }
            }
        }

        // wait for out-connectors to finish
        while out_connecting_futures.next().await.is_some() {}

        // stop peer info db
        self.peer_info_db.stop().await?;

        // Cleanup of connected nodes.
        // drop sender
        drop(self.node_event_tx);
        for (_, (_, node_tx)) in self.active_nodes.drain() {
            // close opened connection.
            trace!("before sending  NodeCommand::Close(ConnectionClosureReason::Normal) from node_tx in network_worker run_loop");
            // send a close command to every node
            // note that we ignore any error here because nodes might have closed by themselves just before
            let _ = node_tx
                .send(NodeCommand::Close(ConnectionClosureReason::Normal))
                .await;
            trace!("after sending  NodeCommand::Close(ConnectionClosureReason::Normal) from node_tx in network_worker run_loop");
        }
        // drain incoming node events
        while self.node_event_rx.recv().await.is_some() {}
        // wait for node join handles
        while let Some(res) = self.node_worker_handles.next().await {
            match res {
                Ok((node_id, Ok(reason))) => {
                    massa_trace!("network.network_worker.cleanup.wait_node.ok", {
                        "node_id": node_id,
                        "reason": reason,
                    });
                }
                Ok((node_id, Err(err))) => {
                    massa_trace!("network.network_worker.cleanup.wait_node.err", {
                        "node_id": node_id,
                        "err": format!("{}", err)
                    });
                }
                Err(err) => {
                    warn!("a node worker panicked: {}", err);
                }
            }
        }

        // wait for all running handshakes
        self.running_handshakes.clear();
        while self.handshake_futures.next().await.is_some() {}
        while self.handshake_peer_list_futures.next().await.is_some() {}
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
    ) -> Result<(), NetworkError> {
        massa_trace!("network_worker.on_handshake_finished.", {
            "node": new_connection_id
        });
        match outcome {
            // a handshake finished, and succeeded
            Ok((new_node_id, socket_reader, socket_writer)) => {
                debug!(
                    "handshake with connection_id={} succeeded => node_id={}",
                    new_connection_id, new_node_id
                );
                massa_trace!("handshake_ok", {
                    "connection_id": new_connection_id,
                    "node_id": new_node_id
                });

                // connection was banned in the meantime
                if !self.running_handshakes.remove(&new_connection_id) {
                    debug!(
                        "connection_id={}, node_id={} peer was banned while handshaking",
                        new_connection_id, new_node_id
                    );
                    massa_trace!("handshake_banned", {
                        "connection_id": new_connection_id,
                        "node_id": new_node_id
                    });
                    self.connection_closed(new_connection_id, ConnectionClosureReason::Normal)
                        .await?;
                    return Ok(());
                }

                match self.active_nodes.entry(new_node_id) {
                    // we already have this node ID
                    hash_map::Entry::Occupied(_) => {
                        debug!(
                            "connection_id={}, node_id={} protocol channel would be redundant",
                            new_connection_id, new_node_id
                        );
                        massa_trace!("node_redundant", {
                            "connection_id": new_connection_id,
                            "node_id": new_node_id
                        });
                        self.connection_closed(new_connection_id, ConnectionClosureReason::Normal)
                            .await?;
                    }
                    // we don't have this node ID
                    hash_map::Entry::Vacant(entry) => {
                        massa_trace!("node_connected", {
                            "connection_id": new_connection_id,
                            "node_id": new_node_id
                        });

                        // Note connection alive.
                        let (ip, _) = self
                            .active_connections
                            .get(&new_connection_id)
                            .ok_or(NetworkError::ActiveConnectionMissing(new_connection_id))?;
                        self.peer_info_db.peer_alive(ip)?;

                        // spawn node_controller_fn
                        let (node_command_tx, node_command_rx) =
                            mpsc::channel::<NodeCommand>(CHANNEL_SIZE);
                        let node_event_tx_clone = self.node_event_tx.clone();
                        let cfg_copy = self.cfg.clone();
                        let node_fn_handle = tokio::spawn(async move {
                            let res = NodeWorker::new(
                                cfg_copy,
                                new_node_id,
                                socket_reader,
                                socket_writer,
                                node_command_rx,
                                node_event_tx_clone,
                            )
                            .run_loop()
                            .await;
                            (new_node_id, res)
                        });
                        entry.insert((new_connection_id, node_command_tx.clone()));
                        self.node_worker_handles.push(node_fn_handle);

                        let res = self
                            .send_network_event(NetworkEvent::NewConnection(new_node_id))
                            .await;

                        // If we failed to send the event to protocol, close the connection.
                        if res.is_err() {
                            let res = node_command_tx
                                .send(NodeCommand::Close(ConnectionClosureReason::Normal))
                                .await;
                            if res.is_err() {
                                massa_trace!(
                                    "network.network_worker.on_handshake_finished", {"err": NetworkError::ChannelError(
                                        "close node command send failed".into(),
                                    ).to_string()}
                                );
                            }
                        }
                    }
                }
            }
            // a handshake failed and sent a list of peers
            Err(NetworkError::HandshakeError(HandshakeErrorType::PeerListReceived(peers))) => {
                // Manage the final of an handshake that send us a list of new peers
                // instead of accepting a connection. Notify to the DB that `to_remove`
                // has failed and merge new `to_add` candidates.
                self.peer_info_db.merge_candidate_peers(&peers)?;
                self.running_handshakes.remove(&new_connection_id);
                self.connection_closed(new_connection_id, ConnectionClosureReason::Failed)
                    .await?;
            }
            // a handshake finished and failed
            Err(err) => {
                debug!(
                    "handshake failed with connection_id={}: {}",
                    new_connection_id, err
                );
                massa_trace!("handshake_failed", {
                    "connection_id": new_connection_id,
                    "err": err.to_string()
                });
                self.running_handshakes.remove(&new_connection_id);
                self.connection_closed(new_connection_id, ConnectionClosureReason::Failed)
                    .await?;
            }
        };
        Ok(())
    }

    async fn connection_closed(
        &mut self,
        id: ConnectionId,
        reason: ConnectionClosureReason,
    ) -> Result<(), NetworkError> {
        let (ip, is_outgoing) = self
            .active_connections
            .remove(&id)
            .ok_or(NetworkError::ActiveConnectionMissing(id))?;
        debug!(
            "connection closed connection_id={}, ip={}, reason={:?}",
            id, ip, reason
        );
        massa_trace!("network_worker.connection_closed", {
            "connection_id": id,
            "ip": ip,
            "reason": reason
        });
        match reason {
            ConnectionClosureReason::Normal => {}
            ConnectionClosureReason::Failed => {
                self.peer_info_db.peer_failed(&ip)?;
            }
            ConnectionClosureReason::Banned => {
                // nothing here, because peer_info_db.peer_banned called in NetworkCommand::Ban
            }
        }
        if is_outgoing {
            self.peer_info_db.out_connection_closed(&ip)?;
        } else {
            self.peer_info_db.in_connection_closed(&ip)?;
        }
        Ok(())
    }

    async fn ban_connection_ids(&mut self, ban_connection_ids: HashSet<ConnectionId>) {
        for ban_conn_id in ban_connection_ids.iter() {
            // remove the connectionId entry in running_handshakes
            self.running_handshakes.remove(ban_conn_id);
        }
        for (conn_id, node_command_tx) in self.active_nodes.values() {
            if ban_connection_ids.contains(conn_id) {
                let res = node_command_tx
                    .send(NodeCommand::Close(ConnectionClosureReason::Banned))
                    .await;
                if res.is_err() {
                    massa_trace!(
                        "network.network_worker.manage_network_command", {"err": NetworkError::ChannelError(
                            "close node command send failed".into(),
                        ).to_string()}
                    );
                }
            };
        }
    }

    /// Manages network commands
    /// Only used inside worker's run_loop
    ///
    /// # Arguments
    /// * cmd : command to process.
    /// * peer_info_db: Database with peer information.
    /// * active_connections: hashmap linking connection id to ipAddr to whether connection is outgoing (true)
    /// * event_tx: channel to send network events out.
    async fn manage_network_command(&mut self, cmd: NetworkCommand) -> Result<(), NetworkError> {
        match cmd {
            NetworkCommand::BanIp(ips) => {
                massa_trace!(
                    "network_worker.manage_network_command receive NetworkCommand::BanIp",
                    { "ips": ips }
                );
                for ip in ips.iter() {
                    self.peer_info_db.peer_banned(ip)?;
                }
                let ban_connection_ids = self
                    .active_connections
                    .iter()
                    .filter_map(|(conn_id, (ip, _))| {
                        if ips.contains(ip) {
                            Some(conn_id)
                        } else {
                            None
                        }
                    })
                    .copied()
                    .collect::<HashSet<_>>();

                self.ban_connection_ids(ban_connection_ids).await
            }
            NetworkCommand::Ban(node) => {
                massa_trace!(
                    "network_worker.manage_network_command receive NetworkCommand::Ban",
                    { "node": node }
                );
                // get all connection IDs to ban
                let mut ban_connection_ids: HashSet<ConnectionId> = HashSet::new();

                // Note: if we can't find the node, there is no need to resend the close event,
                // since protocol will have already removed the node from it's list of active ones.
                if let Some((orig_conn_id, _)) = self.active_nodes.get(&node) {
                    if let Some((orig_ip, _)) = self.active_connections.get(orig_conn_id) {
                        self.peer_info_db.peer_banned(orig_ip)?;
                        for (target_conn_id, (target_ip, _)) in self.active_connections.iter() {
                            if target_ip == orig_ip {
                                ban_connection_ids.insert(*target_conn_id);
                            }
                        }
                    }
                }
                self.ban_connection_ids(ban_connection_ids).await
            }
            NetworkCommand::SendBlockHeader { node, header } => {
                massa_trace!("network_worker.manage_network_command send NodeCommand::SendBlockHeader", {"block_id": header.content.compute_id()?, "header": header, "node": node});
                self.forward_message_to_node_or_resend_close_event(
                    &node,
                    NodeCommand::SendBlockHeader(header),
                )
                .await;
            }
            NetworkCommand::AskForBlocks { list } => {
                for (node, hash_list) in list.into_iter() {
                    massa_trace!(
                        "network_worker.manage_network_command receive NetworkCommand::AskForBlocks",
                        { "hashlist": hash_list, "node": node }
                    );
                    self.forward_message_to_node_or_resend_close_event(
                        &node,
                        NodeCommand::AskForBlocks(hash_list.clone()),
                    )
                    .await;
                }
            }
            NetworkCommand::SendBlock { node, block } => {
                massa_trace!(
                    "network_worker.manage_network_command send NodeCommand::SendBlock",
                    {"hash": block.header.content.compute_hash()?, "block": block, "node": node}
                );
                self.forward_message_to_node_or_resend_close_event(
                    &node,
                    NodeCommand::SendBlock(block),
                )
                .await;
            }
            NetworkCommand::GetPeers(response_tx) => {
                massa_trace!(
                    "network_worker.manage_network_command receive NetworkCommand::GetPeers",
                    {}
                );

                // for each peer get all node id associated to this peer ip.
                let peers: HashMap<IpAddr, Peer> = self
                    .peer_info_db
                    .get_peers()
                    .iter()
                    .map(|(peer_ip_addr, peer)| {
                        (
                            *peer_ip_addr,
                            Peer {
                                peer_info: *peer,
                                active_nodes: self
                                    .active_connections
                                    .iter()
                                    .filter(|(_, (ip_addr, _))| &peer.ip == ip_addr)
                                    .filter_map(|(out_conn_id, (_, out_going))| {
                                        self.active_nodes
                                            .iter()
                                            .filter_map(|(node_id, (conn_id, _))| {
                                                if out_conn_id == conn_id {
                                                    Some(node_id)
                                                } else {
                                                    None
                                                }
                                            })
                                            .next()
                                            .map(|node_id| (*node_id, *out_going))
                                    })
                                    .collect(),
                            },
                        )
                    })
                    .collect();

                // HashMap<NodeId, (ConnectionId, mpsc::Sender<NodeCommand>)
                if response_tx
                    .send(Peers {
                        peers,
                        our_node_id: self.self_node_id,
                    })
                    .is_err()
                {
                    warn!("network: could not send GetPeersChannelError upstream");
                }
            }
            NetworkCommand::GetBootstrapPeers(response_tx) => {
                massa_trace!(
                    "network_worker.manage_network_command receive NetworkCommand::GetBootstrapPeers",
                    {}
                );
                let peer_list = self.peer_info_db.get_advertisable_peer_ips();
                if response_tx.send(BootstrapPeers(peer_list)).is_err() {
                    warn!("network: could not send GetBootstrapPeers response upstream");
                }
            }
            NetworkCommand::BlockNotFound { node, block_id } => {
                massa_trace!(
                    "network_worker.manage_network_command receive NetworkCommand::BlockNotFound",
                    { "block_id": block_id, "node": node }
                );
                self.forward_message_to_node_or_resend_close_event(
                    &node,
                    NodeCommand::BlockNotFound(block_id),
                )
                .await;
            }
            NetworkCommand::SendOperations { node, operations } => {
                massa_trace!(
                    "network_worker.manage_network_command receive NetworkCommand::SendOperations",
                    { "node": node, "operations": operations }
                );
                self.forward_message_to_node_or_resend_close_event(
                    &node,
                    NodeCommand::SendOperations(operations),
                )
                .await;
            }
            NetworkCommand::SendEndorsements { node, endorsements } => {
                massa_trace!(
                    "network_worker.manage_network_command receive NetworkCommand::SendEndorsements",
                    { "node": node, "endorsements": endorsements }
                );
                self.forward_message_to_node_or_resend_close_event(
                    &node,
                    NodeCommand::SendEndorsements(endorsements),
                )
                .await;
            }
            NetworkCommand::NodeSignMessage { msg, response_tx } => {
                massa_trace!(
                    "network_worker.manage_network_command receive NetworkCommand::NodeSignMessage",
                    { "mdg": msg }
                );
                let signature = sign(&Hash::compute_from(&msg), &self.private_key)?;
                let public_key = derive_public_key(&self.private_key);
                if response_tx
                    .send(PubkeySig {
                        public_key,
                        signature,
                    })
                    .is_err()
                {
                    warn!("network: could not send NodeSignMessage response upstream");
                }
            }
            NetworkCommand::Unban(ip) => self.peer_info_db.unban(ip).await?,
            NetworkCommand::GetStats { response_tx } => {
                let res = NetworkStats {
                    in_connection_count: self.peer_info_db.active_in_nonbootstrap_connections
                        as u64, // TODO: add bootstrap connections ... see #1312
                    out_connection_count: self.peer_info_db.active_out_nonbootstrap_connections
                        as u64, // TODO: add bootstrap connections ... see #1312
                    known_peer_count: self.peer_info_db.peers.len() as u64,
                    banned_peer_count: self
                        .peer_info_db
                        .peers
                        .iter()
                        .filter(|(_, p)| p.banned)
                        .fold(0, |acc, _| acc + 1),
                    active_node_count: self.active_nodes.len() as u64,
                };
                if response_tx.send(res).is_err() {
                    warn!("network: could not send NodeSignMessage response upstream");
                }
            }
        }
        Ok(())
    }

    /// Forward a message to a node worker. If it fails, notify upstream about connection closure.
    async fn forward_message_to_node_or_resend_close_event(
        &mut self,
        node: &NodeId,
        message: NodeCommand,
    ) {
        if let Some((_, node_command_tx)) = self.active_nodes.get(node) {
            if node_command_tx.send(message).await.is_err() {
                debug!(
                    "{}",
                    NetworkError::ChannelError("contact with node worker lost while trying to send it a message. Probably a peer disconnect.".into())
                );
            };
        } else {
            // We probably weren't able to send this event previously,
            // retry it now.
            let _ = self
                .send_network_event(NetworkEvent::ConnectionClosed(*node))
                .await;
        }
    }

    /// Manages out connection
    /// Only used inside worker's run_loop
    ///
    /// # Arguments
    /// * res : (reader, writer) in a result coming out of out_connecting_futures
    /// * ip_addr: distant address we are trying to reach.
    /// * cur_connection_id : connection id of the node we are trying to reach
    async fn manage_out_connections(
        &mut self,
        res: tokio::io::Result<(ReadHalf, WriteHalf)>,
        ip_addr: IpAddr,
        cur_connection_id: &mut ConnectionId,
    ) -> Result<(), NetworkError> {
        match res {
            Ok((reader, writer)) => {
                if self
                    .peer_info_db
                    .try_out_connection_attempt_success(&ip_addr)?
                {
                    // outgoing connection established
                    let connection_id = *cur_connection_id;
                    debug!(
                        "out connection towards ip={} established => connection_id={}",
                        ip_addr, connection_id
                    );
                    massa_trace!("out_connection_established", {
                        "ip": ip_addr,
                        "connection_id": connection_id
                    });
                    cur_connection_id.0 += 1;
                    self.active_connections
                        .insert(connection_id, (ip_addr, true));
                    self.manage_successfull_connection(connection_id, reader, writer)?;
                } else {
                    debug!("out connection towards ip={} refused", ip_addr);
                    massa_trace!("out_connection_refused", { "ip": ip_addr });
                }
            }
            Err(err) => {
                debug!(
                    "outgoing connection attempt towards ip={} failed: {}",
                    ip_addr, err
                );
                massa_trace!("out_connection_attempt_failed", {
                    "ip": ip_addr,
                    "err": err.to_string()
                });
                self.peer_info_db.out_connection_attempt_failed(&ip_addr)?;
            }
        }
        Ok(())
    }

    /// Manages in connection
    /// Only used inside worker's run_loop
    ///
    /// Try a connection with an incomming node, if success insert the remote
    /// address at the index `connection_id` inside `self.active_connections`
    /// and call `self.manage_successfull_connection`
    ///
    /// If the connection failed with `MaxPeersConnectionReached`, mock the
    /// handshake and send a list of advertisable peer ips.
    ///
    /// # Arguments
    /// * res : (reader, writer, socketAddr) in a result coming out of the listener
    /// * cur_connection_id : connection id of the node we are trying to reach
    async fn manage_in_connections(
        &mut self,
        res: std::io::Result<(ReadHalf, WriteHalf, SocketAddr)>,
        cur_connection_id: &mut ConnectionId,
    ) -> Result<(), NetworkError> {
        match res {
            Ok((reader, writer, remote_addr)) => {
                match self.peer_info_db.try_new_in_connection(&remote_addr.ip()) {
                    Ok(_) => {
                        let connection_id = *cur_connection_id;
                        debug!(
                            "inbound connection from addr={} succeeded => connection_id={}",
                            remote_addr, connection_id
                        );
                        massa_trace!("in_connection_established", {
                            "ip": remote_addr.ip(),
                            "connection_id": connection_id
                        });
                        cur_connection_id.0 += 1;
                        self.active_connections
                            .insert(connection_id, (remote_addr.ip(), false));
                        self.manage_successfull_connection(connection_id, reader, writer)?;
                    }
                    Err(NetworkError::PeerConnectionError(
                        NetworkConnectionErrorType::MaxPeersConnectionReached(_),
                    )) => self.try_send_peer_list_in_handshake(reader, writer, remote_addr),
                    Err(_) => {
                        debug!("inbound connection from addr={} refused", remote_addr);
                        massa_trace!("in_connection_refused", {"ip": remote_addr.ip()});
                    }
                }
            }
            Err(err) => {
                debug!("connection accept failed: {}", err);
                massa_trace!("in_connection_failed", {"err": err.to_string()});
            }
        }
        Ok(())
    }

    /// Start to mock a handshake and try to send a message with a list of
    /// peers.
    /// The function is used while `manage_in_connections()` if the current
    /// node reach the number of maximum in connection.
    ///
    /// Timeout if the connection as not been done really quickly resolved
    /// but don't throw the timeout error.
    ///
    /// ```txt
    /// In connection node            Curr node
    ///        |                          |
    ///        |------------------------->|      : Try to connect but the
    ///        |                          |        curr node reach the max
    ///        |                          |        number of node.
    ///        |                          |      : Connection success anyway
    ///        |                          |        and the in connection enter
    ///        |<------------------------>|        in `HandshakeWorker::run()`
    ///        |  symetric read & write   |
    ///```
    ///
    /// In the `symetric read & write` the current node simulate a handshake
    /// managed by the *connection node* in `HandshakeWorker::run()`, the
    /// current node send a ListPeer as a message.
    ///
    /// Spawn a future in `self.handshake_peer_list_futures` managed by the
    /// main loop.
    fn try_send_peer_list_in_handshake(
        &self,
        reader: ReadHalf,
        writer: WriteHalf,
        remote_addr: SocketAddr,
    ) {
        debug!(
            "Maximum connection reached. Inbound connection from addr={} refused, try to send a list of peers",
            remote_addr
        );
        if self.cfg.max_in_connection_overflow > self.handshake_peer_list_futures.len() {
            let msg = Message::PeerList(self.peer_info_db.get_advertisable_peer_ips());
            let timeout = self.cfg.peer_list_send_timeout.to_duration();
            self.handshake_peer_list_futures
                .push(tokio::spawn(async move {
                    let mut writer = WriteBinder::new(writer);
                    let mut reader = ReadBinder::new(reader);
                    match tokio::time::timeout(
                        timeout,
                        futures::future::try_join(writer.send(&msg), reader.next()),
                    )
                    .await
                    {
                        Ok(Err(e)) => debug!("Ignored network error on send peerlist={}", e),
                        Err(_) => debug!("Ignored timeout error on send peerlist"),
                        _ => (),
                    }
                }));
        }
    }

    /// Manage a successful incomming and outgoing connection,
    /// Check if we're not already running an handshake for `connection_id` by inserting the connection id in
    /// `self.running_handshakes`
    /// Add a new handshake to perform in `self.handshake_futures` to be handle in the main loop.
    ///
    /// Return an hanshake error if connection already running/waiting
    fn manage_successfull_connection(
        &mut self,
        connection_id: ConnectionId,
        reader: ReadHalf,
        writer: WriteHalf,
    ) -> Result<(), NetworkError> {
        if !self.running_handshakes.insert(connection_id) {
            return Err(NetworkError::HandshakeError(
                HandshakeErrorType::HandshakeIdAlreadyExist(format!("{}", connection_id)),
            ));
        }
        self.handshake_futures.push(HandshakeWorker::spawn(
            reader,
            writer,
            self.self_node_id,
            self.private_key,
            self.cfg.connect_timeout,
            self.version,
            connection_id,
        ));
        Ok(())
    }

    /// Manages node events.
    /// Only used by the worker.
    ///
    /// # Argument
    /// * evt: optional node event to process.
    async fn on_node_event(&mut self, evt: NodeEvent) -> Result<(), NetworkError> {
        match evt {
            // received a list of peers
            NodeEvent(from_node_id, NodeEventType::ReceivedPeerList(lst)) => {
                debug!(
                    "node_id={} sent us a peer list ({} ips)",
                    from_node_id,
                    lst.len()
                );
                massa_trace!("peer_list_received", {
                    "node_id": from_node_id,
                    "ips": lst
                });
                self.peer_info_db.merge_candidate_peers(&lst)?;
            }
            NodeEvent(from_node_id, NodeEventType::ReceivedBlock(data)) => {
                massa_trace!(
                    "network_worker.on_node_event receive NetworkEvent::ReceivedBlock",
                    {"block_id": data.header.content.compute_id()?, "block": data, "node": from_node_id}
                );
                let _ = self
                    .send_network_event(NetworkEvent::ReceivedBlock {
                        node: from_node_id,
                        block: data,
                    })
                    .await;
            }
            NodeEvent(from_node_id, NodeEventType::ReceivedAskForBlocks(list)) => {
                let _ = self
                    .send_network_event(NetworkEvent::AskedForBlocks {
                        node: from_node_id,
                        list,
                    })
                    .await;
            }
            NodeEvent(source_node_id, NodeEventType::ReceivedBlockHeader(header)) => {
                massa_trace!(
                    "network_worker.on_node_event receive NetworkEvent::ReceivedBlockHeader",
                    {"hash": header.content.compute_hash()?, "header": header, "node": source_node_id}
                );
                let _ = self
                    .send_network_event(NetworkEvent::ReceivedBlockHeader {
                        source_node_id,
                        header,
                    })
                    .await;
            }
            // asked peer list
            NodeEvent(from_node_id, NodeEventType::AskedPeerList) => {
                debug!("node_id={} asked us for peer list", from_node_id);
                massa_trace!("node_asked_peer_list", { "node_id": from_node_id });
                let peer_list = self.peer_info_db.get_advertisable_peer_ips();
                if let Some((_, node_command_tx)) = self.active_nodes.get(&from_node_id) {
                    let res = node_command_tx
                        .send(NodeCommand::SendPeerList(peer_list))
                        .await;
                    if res.is_err() {
                        debug!(
                            "{}",
                            NetworkError::ChannelError(
                                "node command send send_peer_list failed".into(),
                            )
                        );
                    }
                } else {
                    massa_trace!("node asked us for peer list and disappeared", {
                        "node_id": from_node_id
                    })
                }
            }

            NodeEvent(node, NodeEventType::BlockNotFound(block_id)) => {
                massa_trace!(
                    "network_worker.on_node_event receive NetworkEvent::BlockNotFound",
                    { "id": block_id }
                );
                let _ = self
                    .send_network_event(NetworkEvent::BlockNotFound { node, block_id })
                    .await;
            }
            NodeEvent(node, NodeEventType::ReceivedOperations(operations)) => {
                massa_trace!(
                    "network_worker.on_node_event receive NetworkEvent::ReceivedOperations",
                    { "operations": operations }
                );
                let _ = self
                    .send_network_event(NetworkEvent::ReceivedOperations { node, operations })
                    .await;
            }
            NodeEvent(node, NodeEventType::ReceivedEndorsements(endorsements)) => {
                massa_trace!(
                    "network_worker.on_node_event receive NetworkEvent::ReceivedEndorsements",
                    { "endorsements": endorsements }
                );
                let _ = self
                    .send_network_event(NetworkEvent::ReceivedEndorsements { node, endorsements })
                    .await;
            }
        }
        Ok(())
    }
}
