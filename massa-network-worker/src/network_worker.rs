// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! The network worker actually does the job of managing connections
use super::{
    handshake_worker::HandshakeReturnType, node_worker::NodeWorker, peer_info_database::*,
};
use crate::{
    binders::{ReadBinder, WriteBinder},
    handshake_worker::HandshakeWorker,
    messages::Message,
    network_event::EventSender,
};
use futures::{stream::FuturesUnordered, StreamExt};
use massa_logging::massa_trace;
use massa_models::{constants::CHANNEL_SIZE, node::NodeId, SerializeCompact, Version};
use massa_network_exports::{
    ConnectionClosureReason, ConnectionId, Establisher, HandshakeErrorType, Listener,
    NetworkCommand, NetworkConnectionErrorType, NetworkError, NetworkEvent,
    NetworkManagementCommand, NetworkSettings, NodeCommand, NodeEvent, NodeEventType, ReadHalf,
    WriteHalf,
};
use massa_signature::{derive_public_key, PrivateKey};
use massa_storage::Storage;
use std::{
    collections::{hash_map, HashMap, HashSet},
    net::{IpAddr, SocketAddr},
};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, trace, warn};

/// Real job is done by network worker
pub struct NetworkWorker {
    /// Network configuration.
    cfg: NetworkSettings,
    /// Our private key.
    pub(crate) private_key: PrivateKey,
    /// Our node id.
    pub(crate) self_node_id: NodeId,
    /// Listener part of the establisher.
    listener: Listener,
    /// The connection establisher.
    establisher: Establisher,
    /// Database with peer information.
    pub(crate) peer_info_db: PeerInfoDatabase,
    /// Receiver for network commands
    controller_command_rx: mpsc::Receiver<NetworkCommand>,
    /// Receiver for network management commands
    controller_manager_rx: mpsc::Receiver<NetworkManagementCommand>,
    /// Set of connection id of node with running handshake.
    pub(crate) running_handshakes: HashSet<ConnectionId>,
    /// Running handshakes futures.
    handshake_futures: FuturesUnordered<JoinHandle<(ConnectionId, HandshakeReturnType)>>,
    /// Running handshakes that send a list of peers.
    handshake_peer_list_futures: FuturesUnordered<JoinHandle<()>>,
    /// Receiving channel for node events.
    node_event_rx: mpsc::Receiver<NodeEvent>,
    /// Ids of active nodes mapped to Connection id, node command sender and handle on the associated node worker.
    pub(crate) active_nodes: HashMap<NodeId, (ConnectionId, mpsc::Sender<NodeCommand>)>,
    /// Node worker handles
    node_worker_handles:
        FuturesUnordered<JoinHandle<(NodeId, Result<ConnectionClosureReason, NetworkError>)>>,
    /// Map of connection to ip, `is_outgoing`.
    pub(crate) active_connections: HashMap<ConnectionId, (IpAddr, bool)>,
    /// Shared storage.
    storage: Storage,
    /// Node version
    version: Version,
    /// Event sender
    pub(crate) event: EventSender,
}

pub struct NetworkWorkerChannels {
    pub controller_command_rx: mpsc::Receiver<NetworkCommand>,
    pub controller_event_tx: mpsc::Sender<NetworkEvent>,
    pub controller_manager_rx: mpsc::Receiver<NetworkManagementCommand>,
}

impl NetworkWorker {
    /// Creates a new `NetworkWorker`
    ///
    /// # Arguments
    /// * `cfg`: Network configuration.
    /// * `listener`: Listener part of the establisher.
    /// * `establisher`: The connection establisher.
    /// * `peer_info_db`: Database with peer information.
    /// * `controller_command_rx`: Channel receiving network commands.
    /// * `controller_event_tx`: Channel sending out network events.
    /// * `controller_manager_rx`: Channel receiving network management commands.
    #[allow(clippy::too_many_arguments)]
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
        storage: Storage,
        version: Version,
    ) -> NetworkWorker {
        let public_key = derive_public_key(&private_key);
        let self_node_id = NodeId(public_key);

        let (node_event_tx, node_event_rx) = mpsc::channel::<NodeEvent>(CHANNEL_SIZE);
        let max_wait_event = cfg.max_send_wait.to_duration();
        NetworkWorker {
            cfg,
            self_node_id,
            private_key,
            listener,
            establisher,
            peer_info_db,
            controller_command_rx,
            event: EventSender::new(controller_event_tx, node_event_tx, max_wait_event),
            controller_manager_rx,
            running_handshakes: HashSet::new(),
            handshake_futures: FuturesUnordered::new(),
            handshake_peer_list_futures: FuturesUnordered::new(),
            node_event_rx,
            active_nodes: HashMap::new(),
            node_worker_handles: FuturesUnordered::new(),
            active_connections: HashMap::new(),
            storage,
            version,
        }
    }

    /// Runs the main loop of the network worker
    /// There is a `tokio::select!` inside the loop
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
                        .event.send(NetworkEvent::ConnectionClosed(node_id))
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
        self.event.drop();
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
    /// * `new_connection_id`: connection id of the connection that should be established here.
    /// * `outcome`: result returned by a handshake.
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
                        let node_event_tx_clone = self.event.clone_node_sender();
                        let cfg_copy = self.cfg.clone();
                        let storage = self.storage.clone();
                        let node_fn_handle = tokio::spawn(async move {
                            let res = NodeWorker::new(
                                cfg_copy,
                                new_node_id,
                                socket_reader,
                                socket_writer,
                                node_command_rx,
                                node_event_tx_clone,
                                storage,
                            )
                            .run_loop()
                            .await;
                            (new_node_id, res)
                        });
                        entry.insert((new_connection_id, node_command_tx.clone()));
                        self.node_worker_handles.push(node_fn_handle);

                        let res = self
                            .event
                            .send(NetworkEvent::NewConnection(new_node_id))
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

    /// Manages network commands
    /// Only used inside worker's `run_loop`
    ///
    /// # Arguments
    /// * `cmd` : command to process.
    /// * `peer_info_db`: Database with peer information.
    /// * `active_connections`: hashmap linking connection id to `ÌpAddr` to
    ///   whether connection is outgoing (true)
    /// * `event_tx`: channel to send network events out.
    ///
    /// # Command implementation
    /// Some of the commands are just forwarded to the `NodeWorker` that manage
    /// the real connection between nodes. Some other commands has an impact on
    /// the current worker.
    ///
    /// Whatever the behavior of the command, we better have to look at
    /// `network_cmd_impl.rs` where the commands are implemented.
    ///
    /// ex: `NetworkCommand::AskForBlocks` => `on_ask_bfor_block_cmd(...)`
    async fn manage_network_command(&mut self, cmd: NetworkCommand) -> Result<(), NetworkError> {
        use crate::network_cmd_impl::*;
        match cmd {
            NetworkCommand::BanIp(ips) => on_ban_ip_cmd(self, ips).await?,
            NetworkCommand::Ban(node) => on_ban_cmd(self, node).await?,
            NetworkCommand::SendBlockHeader { node, block_id } => {
                on_send_block_header_cmd(self, node, block_id).await?
            }
            NetworkCommand::AskForBlocks { list } => on_ask_for_block_cmd(self, list).await,
            NetworkCommand::SendBlock { node, block_id } => {
                on_send_block_cmd(self, node, block_id).await?
            }
            NetworkCommand::GetPeers(response_tx) => on_get_peers_cmd(self, response_tx).await,
            NetworkCommand::GetBootstrapPeers(response_tx) => {
                on_get_bootstrap_peers_cmd(self, response_tx).await
            }
            NetworkCommand::BlockNotFound { node, block_id } => {
                on_block_not_found_cmd(self, node, block_id).await
            }
            NetworkCommand::SendOperations { node, operations } => {
                on_send_operations_cmd(self, node, operations).await
            }
            NetworkCommand::SendOperationAnnouncements { to_node, batch } => {
                on_send_operation_batches_cmd(self, to_node, batch).await
            }
            NetworkCommand::AskForOperations { to_node, wishlist } => {
                on_ask_for_operations_cmd(self, to_node, wishlist).await
            }
            NetworkCommand::SendEndorsements { node, endorsements } => {
                on_send_endorsements_cmd(self, node, endorsements).await
            }
            NetworkCommand::NodeSignMessage { msg, response_tx } => {
                on_node_sign_message_cmd(self, msg, response_tx).await?
            }
            NetworkCommand::Unban(ip) => on_unban_cmd(self, ip).await?,
            NetworkCommand::GetStats { response_tx } => on_get_stats_cmd(self, response_tx).await,
            NetworkCommand::Whitelist(ips) => on_whitelist_cmd(self, ips).await?,
            NetworkCommand::RemoveFromWhitelist(ips) => {
                on_remove_from_whitelist_cmd(self, ips).await?
            }
        };
        Ok(())
    }

    /// Manages out connection
    /// Only used inside worker's `run_loop`
    ///
    /// # Arguments
    /// * `res`: `(reader, writer)` in a result coming out of `out_connecting_futures`
    /// * `ip_addr`: distant address we are trying to reach.
    /// * `cur_connection_id`: connection id of the node we are trying to reach
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
                    self.manage_successful_connection(connection_id, reader, writer)?;
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
    /// Only used inside worker's `run_loop`
    ///
    /// Try a connection with an incoming node, if success insert the remote
    /// address at the index `connection_id` inside `self.active_connections`
    /// and call `self.manage_successful_connection`
    ///
    /// If the connection failed with `MaxPeersConnectionReached`, mock the
    /// handshake and send a list of advertisable peer ips.
    ///
    /// # Arguments
    /// * `re` : `(reader, writer, socketAddr)` in a result coming out of the listener
    /// * `cur_connection_id`: connection id of the node we are trying to reach
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
                        self.manage_successful_connection(connection_id, reader, writer)?;
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
    ///        |  symmetric read & write   |
    ///```
    ///
    /// In the `symmetric read & write` the current node simulate a handshake
    /// managed by the *connection node* in `HandshakeWorker::run()`, the
    /// current node send a `ListPeer` as a message.
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
                        futures::future::try_join(
                            writer.send(&msg.to_bytes_compact().unwrap()),
                            reader.next(),
                        ),
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

    /// Manage a successful incoming and outgoing connection,
    /// Check if we're not already running an handshake for `connection_id` by inserting the connection id in
    /// `self.running_handshakes`
    /// Add a new handshake to perform in `self.handshake_futures` to be handle in the main loop.
    ///
    /// Return an handshake error if connection already running/waiting
    fn manage_successful_connection(
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
    /// * `evt`: optional node event to process.
    async fn on_node_event(&mut self, evt: NodeEvent) -> Result<(), NetworkError> {
        use crate::network_event::*;
        match evt {
            // received a list of peers
            NodeEvent(from_node_id, NodeEventType::ReceivedPeerList(lst)) => {
                event_impl::on_received_peer_list(self, from_node_id, &lst)?
            }
            NodeEvent(from_node_id, NodeEventType::ReceivedBlock(block, serialized)) => {
                event_impl::on_received_block(self, from_node_id, block, serialized).await?
            }
            NodeEvent(from_node_id, NodeEventType::ReceivedAskForBlocks(list)) => {
                event_impl::on_received_ask_for_blocks(self, from_node_id, list).await
            }
            NodeEvent(source_node_id, NodeEventType::ReceivedBlockHeader(header)) => {
                event_impl::on_received_block_header(self, source_node_id, header).await?
            }
            NodeEvent(from_node_id, NodeEventType::AskedPeerList) => {
                event_impl::on_asked_peer_list(self, from_node_id).await?
            }
            NodeEvent(node, NodeEventType::BlockNotFound(block_id)) => {
                event_impl::on_block_not_found(self, node, block_id).await
            }
            NodeEvent(node, NodeEventType::ReceivedOperations(operations)) => {
                event_impl::on_received_operations(self, node, operations).await
            }
            NodeEvent(node, NodeEventType::ReceivedEndorsements(endorsements)) => {
                event_impl::on_received_endorsements(self, node, endorsements).await
            }
            NodeEvent(node, NodeEventType::ReceivedOperationAnnouncements(operation_ids)) => {
                event_impl::on_received_operations_annoncement(self, node, operation_ids).await
            }
            NodeEvent(node, NodeEventType::ReceivedAskForOperations(operation_ids)) => {
                event_impl::on_received_ask_for_operations(self, node, operation_ids).await
            }
        }
        Ok(())
    }
}
