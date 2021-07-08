//! The network worker actually does the job of managing connections
//! That's why it's ... a worker ! 🦀
use super::{
    common::{ConnectionClosureReason, ConnectionId},
    config::{NetworkConfig, CHANNEL_SIZE},
    establisher::{Establisher, Listener, ReadHalf, WriteHalf},
    handshake_worker::{HandshakeReturnType, HandshakeWorker},
    node_worker::{NodeCommand, NodeEvent, NodeEventType, NodeWorker},
    peer_info_database::*,
};
use crate::common::NodeId;
use crate::error::{CommunicationError, HandshakeErrorType};
use crate::logging::debug;
use crypto::hash::Hash;
use crypto::signature::PrivateKey;
use futures::{stream::FuturesUnordered, StreamExt};
use hang_monitor::{
    HangAnnotation, HangMonitorCommandSender, MonitoredComponentId, NetworkHangAnnotation,
    ProtocolHangAnnotation,
};
use models::{Block, BlockHeader, SerializationContext};
use std::{
    collections::{hash_map, HashMap, HashSet},
    net::{IpAddr, SocketAddr},
};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

/// Commands that the worker can execute
#[derive(Debug)]
pub enum NetworkCommand {
    /// Ask for a block from a node.
    AskForBlock {
        node: NodeId,
        hash: Hash,
    },
    /// Send that block to node.
    SendBlock {
        node: NodeId,
        block: Block,
    },
    /// Send a header to a node.
    SendBlockHeader {
        node: NodeId,
        header: BlockHeader,
    },
    GetPeers(oneshot::Sender<HashMap<IpAddr, PeerInfo>>),
    Ban(NodeId),
    BlockNotFound {
        node: NodeId,
        hash: Hash,
    },
}

impl From<&NetworkCommand> for NetworkHangAnnotation {
    fn from(msg: &NetworkCommand) -> Self {
        match msg {
            NetworkCommand::AskForBlock { .. } => Self::AskForBlock,
            NetworkCommand::SendBlock { .. } => Self::SendBlock,
            NetworkCommand::SendBlockHeader { .. } => Self::SendBlockHeader,
            NetworkCommand::GetPeers { .. } => Self::GetPeers,
            NetworkCommand::Ban(_) => Self::Ban,
            NetworkCommand::BlockNotFound { .. } => Self::CommandBlockNotFound,
        }
    }
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
        header: BlockHeader,
    },
    /// Someone ask for block with given header hash.
    AskedForBlock {
        node: NodeId,
        hash: Hash,
    },
    /// That node does not have this block
    BlockNotFound {
        node: NodeId,
        hash: Hash,
    },
}

impl From<&NetworkEvent> for ProtocolHangAnnotation {
    fn from(msg: &NetworkEvent) -> Self {
        match msg {
            NetworkEvent::NewConnection(_) => Self::NewConnection,
            NetworkEvent::ConnectionClosed(_) => Self::ConnectionClosed,
            NetworkEvent::ReceivedBlock { .. } => Self::ReceivedBlock,
            NetworkEvent::ReceivedBlockHeader { .. } => Self::ReceivedBlockHeader,
            NetworkEvent::AskedForBlock { .. } => Self::AskedForBlock,
            NetworkEvent::BlockNotFound { .. } => Self::BlockNotFound,
        }
    }
}

#[derive(Debug)]
pub enum NetworkManagementCommand {}

/// Real job is done by network worker
pub struct NetworkWorker {
    /// Network configuration.
    cfg: NetworkConfig,
    // Serialization context
    serialization_context: SerializationContext,
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
    /// Channel for sending node events.
    node_event_tx: mpsc::Sender<NodeEvent>,
    /// Receiving channel for node events.
    node_event_rx: mpsc::Receiver<NodeEvent>,
    /// Ids of active nodes mapped to Connection id, node command sender and handle on the associated node worker.
    active_nodes: HashMap<
        NodeId,
        (
            ConnectionId,
            mpsc::Sender<NodeCommand>,
            JoinHandle<Result<(), CommunicationError>>,
        ),
    >,
    /// Map of connection to ip, is_outgoing.
    active_connections: HashMap<ConnectionId, (IpAddr, bool)>,
    /// Hang monitor.
    hang_monitor: Option<HangMonitorCommandSender>,
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
        cfg: NetworkConfig,
        serialization_context: SerializationContext,
        private_key: PrivateKey,
        self_node_id: NodeId,
        listener: Listener,
        establisher: Establisher,
        peer_info_db: PeerInfoDatabase,
        controller_command_rx: mpsc::Receiver<NetworkCommand>,
        controller_event_tx: mpsc::Sender<NetworkEvent>,
        controller_manager_rx: mpsc::Receiver<NetworkManagementCommand>,
        hang_monitor: Option<HangMonitorCommandSender>,
    ) -> NetworkWorker {
        let (node_event_tx, node_event_rx) = mpsc::channel::<NodeEvent>(CHANNEL_SIZE);
        NetworkWorker {
            cfg,
            serialization_context,
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
            node_event_tx,
            node_event_rx,
            active_nodes: HashMap::new(),
            active_connections: HashMap::new(),
            hang_monitor,
        }
    }

    /// Runs the main loop of the network_worker
    /// There is a tokio::select! insside the loop
    pub async fn run_loop(mut self) -> Result<(), CommunicationError> {
        if let Some(monitor) = self.hang_monitor.as_mut() {
            // Register the worker with the hang-monitor.
            monitor
                .register(MonitoredComponentId::Network)
                .await
                .map_err(|_| {
                    CommunicationError::ChannelError("Register with hang-monitor failed".into())
                })?;
        }

        let mut out_connecting_futures = FuturesUnordered::new();
        let mut cur_connection_id = ConnectionId::default();

        // wake up the controller at a regular interval to retry connections
        let mut wakeup_interval = tokio::time::interval(self.cfg.wakeup_interval.to_duration());

        loop {
            {
                // try to connect to candidate IPs
                let candidate_ips = self.peer_info_db.get_out_connection_candidate_ips()?;
                for ip in candidate_ips {
                    debug!("starting outgoing connection attempt towards ip={:?}", ip);
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
            }

            if let Some(monitor) = self.hang_monitor.as_mut() {
                // Notify the hang-monitor we are about to wait on a select.
                monitor.notify_wait().await.map_err(|_| {
                    CommunicationError::ChannelError(
                        "Wait notification to hang-monitor failed".into(),
                    )
                })?;
            }
            tokio::select! {
                // listen to manager commands
                cmd = self.controller_manager_rx.recv() => match cmd {
                    None => break,
                    Some(_) => {}
                },

                // wake up interval
                _ = wakeup_interval.tick() => {},

                // wait for a handshake future to complete
                Some(res) = self.handshake_futures.next() => {
                    let (conn_id, outcome) = res?;
                    self.on_handshake_finished(conn_id, outcome).await?;
                },

                // event received from a node
                evt = self.node_event_rx.recv() => self.on_node_event(
                    evt.ok_or(CommunicationError::ChannelError("node event rx failed".into()))?
                ).await?,

                // peer feedback event
                Some(cmd) = self.controller_command_rx.recv() => self.manage_network_command(
                    cmd,
                ).await?,

                // out-connector event
                Some((ip_addr, res)) = out_connecting_futures.next() => self.manage_out_connections(
                    res,
                    ip_addr,
                    &mut cur_connection_id,
                ).await?,

                // listener socket received
                res = self.listener.accept() => self.manage_in_connections(
                    res,
                    &mut cur_connection_id,
                ).await?
            }
        }

        // wait for out-connectors to finish
        while let Some(_) = out_connecting_futures.next().await {}

        // stop peer info db
        self.peer_info_db.stop().await?;

        // Cleanup of connected nodes.
        {
            let mut node_handle_set = FuturesUnordered::new();
            // drop sender
            drop(self.node_event_tx);
            // gather active node handles
            for (_, (_, node_tx, handle)) in self.active_nodes.drain() {
                //close opened connection.
                node_tx
                    .send(NodeCommand::Close(ConnectionClosureReason::Normal))
                    .await
                    .map_err(|_| {
                        CommunicationError::ChannelError("node close command send failed".into())
                    })?;
                node_handle_set.push(handle);
            }
            // drain incoming node events
            while let Some(_) = self.node_event_rx.recv().await {}
            // wait for node join handles
            while let Some(res) = node_handle_set.next().await {
                match res {
                    Ok(Ok(_)) => (),
                    Ok(Err(err)) => return Err(err),
                    Err(err) => return Err(CommunicationError::TokioTaskJoinError(err)),
                }
            }
        }

        // wait for all running handshakes
        self.running_handshakes.clear();
        while let Some(_) = self.handshake_futures.next().await {}
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
        if let Some(monitor) = self.hang_monitor.as_mut() {
            monitor
                .notify_activity(HangAnnotation::Network(
                    NetworkHangAnnotation::HandshakeFinished,
                ))
                .await
                .map_err(|_| {
                    CommunicationError::ChannelError(
                        "Activity notification to hang-monitor failed".into(),
                    )
                })?;
        }
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
                    self.connection_closed(new_connection_id, ConnectionClosureReason::Normal)
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
                        self.connection_closed(new_connection_id, ConnectionClosureReason::Normal)
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

                        // Note connection alive.
                        let (ip, _) = self.active_connections.get(&new_connection_id).ok_or(
                            CommunicationError::ActiveConnectionMissing(new_connection_id),
                        )?;
                        self.peer_info_db.peer_alive(&ip)?;

                        // spawn node_controller_fn
                        let (node_command_tx, node_command_rx) =
                            mpsc::channel::<NodeCommand>(CHANNEL_SIZE);
                        let node_event_tx_clone = self.node_event_tx.clone();
                        let cfg_copy = self.cfg.clone();
                        let monitor = self.hang_monitor.clone();
                        let node_fn_handle = tokio::spawn(async move {
                            NodeWorker::new(
                                cfg_copy,
                                new_node_id,
                                socket_reader,
                                socket_writer,
                                node_command_rx,
                                node_event_tx_clone,
                                monitor,
                            )
                            .run_loop()
                            .await
                        });
                        entry.insert((new_connection_id, node_command_tx, node_fn_handle));
                        self.controller_event_tx
                            .send(NetworkEvent::NewConnection(new_node_id))
                            .await
                            .map_err(|_| {
                                CommunicationError::ChannelError(
                                    "could not send new connection notification upstream".into(),
                                )
                            })?;
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
                self.connection_closed(new_connection_id, ConnectionClosureReason::Failed)
                    .await?;
            }
        };
        Ok(())
    }

    async fn new_connection(
        &mut self,
        connection_id: ConnectionId,
        reader: ReadHalf,
        writer: WriteHalf,
    ) -> Result<(), CommunicationError> {
        if let Some(monitor) = self.hang_monitor.as_mut() {
            monitor
                .notify_activity(HangAnnotation::Network(
                    NetworkHangAnnotation::NewConnection,
                ))
                .await
                .map_err(|_| {
                    CommunicationError::ChannelError(
                        "Activity notification to hang-monitor failed".into(),
                    )
                })?;
        }

        // add connection ID to running_handshakes
        // launch async handshake_fn(connectionId, socket)
        // add its handle to handshake_futures
        if !self.running_handshakes.insert(connection_id) {
            return Err(CommunicationError::HandshakeError(
                HandshakeErrorType::HandshakeIdAlreadyExistError(format!("{}", connection_id)),
            ));
        }

        debug!("starting handshake with connection_id={:?}", connection_id);
        massa_trace!("handshake_start", { "connection_id": connection_id });

        let self_node_id = self.self_node_id;
        let private_key = self.private_key;
        let message_timeout = self.cfg.message_timeout;
        let connection_id_copy = connection_id.clone();
        let serialization_context = self.serialization_context.clone();
        let handshake_fn_handle = tokio::spawn(async move {
            (
                connection_id_copy,
                HandshakeWorker::new(
                    serialization_context,
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
        Ok(())
    }

    async fn connection_closed(
        &mut self,
        id: ConnectionId,
        reason: ConnectionClosureReason,
    ) -> Result<(), CommunicationError> {
        if let Some(monitor) = self.hang_monitor.as_mut() {
            monitor
                .notify_activity(HangAnnotation::Network(
                    NetworkHangAnnotation::ConnectionClosed,
                ))
                .await
                .map_err(|_| {
                    CommunicationError::ChannelError(
                        "Activity notification to hang-monitor failed".into(),
                    )
                })?;
        }
        let (ip, is_outgoing) = self
            .active_connections
            .remove(&id)
            .ok_or(CommunicationError::ActiveConnectionMissing(id))?;
        debug!(
            "connection closed connedtion_id={:?}, ip={:?}, reason={:?}",
            id, ip, reason
        );
        massa_trace!("connection_closed", {
            "connnection_id": id,
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

    /// Manages network commands
    /// Only used inside worker's run_loop
    ///
    /// # Arguments
    /// * cmd : command to process.
    /// * peer_info_db: Database with peer information.
    /// * active_connections: hashmap linking connection id to ipAddr to wether connection is outgoing (true)
    /// * event_tx: channel to send network events out.
    async fn manage_network_command(
        &mut self,
        cmd: NetworkCommand,
    ) -> Result<(), CommunicationError> {
        if let Some(monitor) = self.hang_monitor.as_mut() {
            monitor
                .notify_activity(HangAnnotation::Network((&cmd).into()))
                .await
                .map_err(|_| {
                    CommunicationError::ChannelError(
                        "Activity notification to hang-monitor failed".into(),
                    )
                })?;
        }
        match cmd {
            NetworkCommand::Ban(node) => {
                // get all connection IDs to ban
                let mut ban_connection_ids: HashSet<ConnectionId> = HashSet::new();
                if let Some((orig_conn_id, _, _)) = self.active_nodes.get(&node) {
                    if let Some((orig_ip, _)) = self.active_connections.get(orig_conn_id) {
                        self.peer_info_db.peer_banned(orig_ip)?;
                        for (target_conn_id, (target_ip, _)) in self.active_connections.iter() {
                            if target_ip == orig_ip {
                                ban_connection_ids.insert(*target_conn_id);
                            }
                        }
                    }
                }
                for ban_conn_id in ban_connection_ids.iter() {
                    // remove the connectionId entry in running_handshakes
                    self.running_handshakes.remove(&ban_conn_id);
                }
                for (conn_id, node_command_tx, _) in self.active_nodes.values() {
                    if ban_connection_ids.contains(conn_id) {
                        node_command_tx
                            .send(NodeCommand::Close(ConnectionClosureReason::Banned))
                            .await
                            .map_err(|_| {
                                CommunicationError::ChannelError(
                                    "close node command send failed".into(),
                                )
                            })?;
                    }
                }
            }
            NetworkCommand::SendBlockHeader { node, header } => {
                if let Some((_, node_command_tx, _)) = self.active_nodes.get_mut(&node) {
                    node_command_tx
                        .send(NodeCommand::SendBlockHeader(header))
                        .await
                        .map_err(|_| {
                            CommunicationError::ChannelError(
                                "send block header node command send failed".into(),
                            )
                        })?;
                }
            }
            NetworkCommand::AskForBlock { node, hash } => {
                if let Some((_, node_command_tx, _)) = self.active_nodes.get_mut(&node) {
                    node_command_tx
                        .send(NodeCommand::AskForBlock(hash))
                        .await
                        .map_err(|_| {
                            CommunicationError::ChannelError(
                                "ask for block node command send failed".into(),
                            )
                        })?;
                }
            }
            NetworkCommand::SendBlock { node, block } => {
                if let Some((_, node_command_tx, _)) = self.active_nodes.get_mut(&node) {
                    node_command_tx
                        .send(NodeCommand::SendBlock(block))
                        .await
                        .map_err(|_| {
                            CommunicationError::ChannelError(
                                "send block node command send failed".into(),
                            )
                        })?;
                }
            }
            NetworkCommand::GetPeers(response_tx) => {
                response_tx
                    .send(self.peer_info_db.get_peers().clone())
                    .map_err(|_| {
                        CommunicationError::ChannelError(
                            "could not send GetPeersChannelError upstream".into(),
                        )
                    })?;
            }
            NetworkCommand::BlockNotFound { node, hash } => {
                if let Some((_, node_command_tx, _)) = self.active_nodes.get_mut(&node) {
                    node_command_tx
                        .send(NodeCommand::BlockNotFound(hash))
                        .await
                        .map_err(|_| {
                            CommunicationError::ChannelError(
                                "send block not found node command send failed".into(),
                            )
                        })?;
                }
            }
        }
        Ok(())
    }

    /// Manages out connection
    /// Only used inside worker's run_loop
    ///
    /// # Arguments
    /// * res : (reader, writer) in a result comming out of out_connecting_futures
    /// * peer_info_db: Database with peer information.
    /// * cur_connection_id : connection id of the node we are trying to reach
    /// * active_connections: hashmap linking connection id to ipAddr to wether connection is outgoing (true)
    /// * event_tx: channel to send network events out.
    async fn manage_out_connections(
        &mut self,
        res: tokio::io::Result<(ReadHalf, WriteHalf)>,
        ip_addr: IpAddr,
        cur_connection_id: &mut ConnectionId,
    ) -> Result<(), CommunicationError> {
        if let Some(monitor) = self.hang_monitor.as_mut() {
            monitor
                .notify_activity(HangAnnotation::Network(
                    NetworkHangAnnotation::ManageOutConnections,
                ))
                .await
                .map_err(|_| {
                    CommunicationError::ChannelError(
                        "Activity notification to hang-monitor failed".into(),
                    )
                })?;
        }
        match res {
            Ok((reader, writer)) => {
                if self
                    .peer_info_db
                    .try_out_connection_attempt_success(&ip_addr)?
                {
                    // outgoing connection established
                    let connection_id = *cur_connection_id;
                    debug!(
                        "out connection towards ip={:?} established => connection_id={:?}",
                        ip_addr, connection_id
                    );
                    massa_trace!("out_connection_established", {
                        "ip": ip_addr,
                        "connection_id": connection_id
                    });
                    cur_connection_id.0 += 1;
                    self.active_connections
                        .insert(connection_id, (ip_addr, true));
                    self.new_connection(connection_id, reader, writer).await?;
                } else {
                    debug!("out connection towards ip={:?} refused", ip_addr);
                    massa_trace!("out_connection_refused", { "ip": ip_addr });
                }
            }
            Err(err) => {
                debug!(
                    "outgoing connection attempt towards ip={:?} failed: {:?}",
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
    /// # Arguments
    /// * res : (reader, writer, socketAddr) in a result comming out of the listener
    /// * peer_info_db: Database with peer information.
    /// * cur_connection_id : connection id of the node we are trying to reach
    /// * active_connections: hashmap linking connection id to ipAddr to wether connection is outgoing (true)
    /// * event_tx: channel to send network events out.
    async fn manage_in_connections(
        &mut self,
        res: std::io::Result<(ReadHalf, WriteHalf, SocketAddr)>,
        cur_connection_id: &mut ConnectionId,
    ) -> Result<(), CommunicationError> {
        if let Some(monitor) = self.hang_monitor.as_mut() {
            monitor
                .notify_activity(HangAnnotation::Network(
                    NetworkHangAnnotation::ManageInConnections,
                ))
                .await
                .map_err(|_| {
                    CommunicationError::ChannelError(
                        "Activity notification to hang-monitor failed".into(),
                    )
                })?;
        }
        match res {
            Ok((reader, writer, remote_addr)) => {
                if self.peer_info_db.try_new_in_connection(&remote_addr.ip())? {
                    let connection_id = *cur_connection_id;
                    debug!(
                        "inbound connection from addr={:?} succeeded => connection_id={:?}",
                        remote_addr, connection_id
                    );
                    massa_trace!("in_connection_established", {
                        "ip": remote_addr.ip(),
                        "connection_id": connection_id
                    });
                    cur_connection_id.0 += 1;
                    self.active_connections
                        .insert(connection_id, (remote_addr.ip(), false));
                    self.new_connection(connection_id, reader, writer).await?;
                } else {
                    debug!("inbound connection from addr={:?} refused", remote_addr);
                    massa_trace!("in_connection_refused", {"ip": remote_addr.ip()});
                }
            }
            Err(err) => {
                debug!("connection accept failed: {:?}", err);
                massa_trace!("in_connection_failed", {"err": err.to_string()});
            }
        }
        Ok(())
    }

    /// Manages node events.
    /// Only used by the worker.
    ///
    /// # Argument
    /// * evt: optional node event to process.
    async fn on_node_event(&mut self, evt: NodeEvent) -> Result<(), CommunicationError> {
        if let Some(monitor) = self.hang_monitor.as_mut() {
            monitor
                .notify_activity(HangAnnotation::Network((&evt).into()))
                .await
                .map_err(|_| {
                    CommunicationError::ChannelError(
                        "Activity notification to hang-monitor failed".into(),
                    )
                })?;
        }
        match evt {
            // received a list of peers
            NodeEvent(from_node_id, NodeEventType::ReceivedPeerList(lst)) => {
                debug!("node_id={:?} sent us a peer list: {:?}", from_node_id, lst);
                massa_trace!("peer_list_received", {
                    "node_id": from_node_id,
                    "ips": lst
                });

                debug!("merging incoming peer list: {:?}", lst);
                massa_trace!("merge_incoming_peer_list", { "ips": lst });
                self.peer_info_db.merge_candidate_peers(&lst)?;
            }
            NodeEvent(from_node_id, NodeEventType::ReceivedBlock(data)) => self
                .controller_event_tx
                .send(NetworkEvent::ReceivedBlock {
                    node: from_node_id,
                    block: data,
                })
                .await
                .map_err(|_| {
                    CommunicationError::ChannelError("receive block event send failed".into())
                })?,
            NodeEvent(from_node_id, NodeEventType::ReceivedAskForBlock(data)) => self
                .controller_event_tx
                .send(NetworkEvent::AskedForBlock {
                    node: from_node_id,
                    hash: data,
                })
                .await
                .map_err(|_| {
                    CommunicationError::ChannelError(
                        "receive asked for block event send failed".into(),
                    )
                })?,
            NodeEvent(source_node_id, NodeEventType::ReceivedBlockHeader(header)) => self
                .controller_event_tx
                .send(NetworkEvent::ReceivedBlockHeader {
                    source_node_id,
                    header,
                })
                .await
                .map_err(|_| {
                    CommunicationError::ChannelError("receive block event send failed".into())
                })?,
            // connection closed
            NodeEvent(from_node_id, NodeEventType::Closed(reason)) => {
                self.controller_event_tx
                    .send(NetworkEvent::ConnectionClosed(from_node_id))
                    .await
                    .map_err(|_| {
                        CommunicationError::ChannelError(
                            "connection closed event send failed".into(),
                        )
                    })?;
                let (connection_id, _, handle) = self
                    .active_nodes
                    .remove(&from_node_id)
                    .ok_or(CommunicationError::MissingNodeError)?;
                info!("protocol channel closed peer_id={:?}", from_node_id);
                massa_trace!("node_closed", {
                    "node_id": from_node_id,
                    "reason": reason
                });
                self.connection_closed(connection_id, reason).await?;
                handle.await??;
            }
            // asked peer list
            NodeEvent(from_node_id, NodeEventType::AskedPeerList) => {
                debug!("node_id={:?} asked us for peer list", from_node_id);
                massa_trace!("node_asked_peer_list", { "node_id": from_node_id });
                let peer_list = self.peer_info_db.get_advertisable_peer_ips();
                let (_, node_command_tx, _) = self
                    .active_nodes
                    .get(&from_node_id)
                    .ok_or(CommunicationError::MissingNodeError)?;
                node_command_tx
                    .send(NodeCommand::SendPeerList(peer_list))
                    .await
                    .map_err(|_| {
                        CommunicationError::ChannelError(
                            "node command send send_peer_list failed".into(),
                        )
                    })?
            }

            NodeEvent(node, NodeEventType::BlockNotFound(hash)) => self
                .controller_event_tx
                .send(NetworkEvent::BlockNotFound { node, hash })
                .await
                .map_err(|_| {
                    CommunicationError::ChannelError("receive block event send failed".into())
                })?,
        }
        Ok(())
    }
}
