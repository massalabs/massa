// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! The network worker actually does the job of managing connections
use super::{
    handshake_worker::HandshakeReturnType, node_worker::NodeWorker, peer_info_database::*,
};
use crate::{
    binders::{ReadBinder, WriteBinder},
    handshake_worker::{start_handshake_manager, HandshakeWorker},
    messages::{Message, MessageDeserializer},
    network_event::EventSender,
};
use crossbeam_channel::{bounded, select, Receiver, Sender};
use futures::{stream::FuturesUnordered, StreamExt};
use massa_logging::massa_trace;
use massa_models::{node::NodeId, version::Version};
use massa_network_exports::{
    ConnectionClosureReason, ConnectionId, Establisher, HandshakeErrorType, Listener,
    NetworkCommand, NetworkConfig, NetworkConnectionErrorType, NetworkError, NetworkEvent,
    NetworkManagementCommand, NodeCommand, NodeEvent, NodeEventType, ReadHalf, WriteHalf,
};
use massa_signature::KeyPair;
use std::thread::{self, JoinHandle};
use std::{
    collections::{hash_map, HashMap, HashSet},
    net::{IpAddr, SocketAddr},
};
use tokio::runtime::{Handle, Runtime};
use tokio::task::JoinHandle as AsyncJoinHandle;
use tracing::debug;

/// Real job is done by network worker
pub struct NetworkWorker {
    /// Network configuration.
    cfg: NetworkConfig,
    /// Our keypair.
    pub(crate) keypair: KeyPair,
    /// Our node id.
    pub(crate) self_node_id: NodeId,
    /// Listener part of the establisher.
    listener: Option<Listener>,
    /// The connection establisher.
    establisher: Establisher,
    /// Database with peer information.
    pub(crate) peer_info_db: PeerInfoDatabase,
    /// Receiver for network commands
    controller_command_rx: Receiver<NetworkCommand>,
    /// Receiver for network management commands
    controller_manager_rx: Receiver<NetworkManagementCommand>,
    /// Set of connection id of node with running handshake.
    pub(crate) running_handshakes: HashSet<ConnectionId>,
    /// Running handshakes that send a list of peers.
    handshake_peer_list_futures: HashMap<IpAddr, AsyncJoinHandle<()>>,
    /// Receiving channel for node events.
    node_event_rx: Receiver<NodeEvent>,
    /// Ids of active nodes mapped to Connection id, node command sender and handle on the associated node worker.
    pub(crate) active_nodes: HashMap<NodeId, (ConnectionId, Sender<NodeCommand>, JoinHandle<()>)>,
    /// Map of connection to ip, `is_outgoing`.
    pub(crate) active_connections: HashMap<ConnectionId, (IpAddr, bool)>,
    /// Node version
    version: Version,
    /// Event sender
    pub(crate) event: EventSender,
    /// Tokio runtime.
    runtime: Runtime,
    /// A channel to receive the results of running a handshake.
    connections_rx: Receiver<(ConnectionId, HandshakeReturnType)>,
    /// A channel to send handshake workers to be run on by the handshake manager. 
    handshake_tx: Sender<(ConnectionId, HandshakeWorker)>,
    /// The handle to the handshake manager.
    handshake_manager_join_handle: JoinHandle<()>,
    /// A channel to receive the result of closing a node worker.
    node_result_rx: Receiver<(NodeId, Result<ConnectionClosureReason, NetworkError>)>,
    /// A channel, cloned for each node worker, to send the result of closing a node worker.
    node_result_tx: Sender<(NodeId, Result<ConnectionClosureReason, NetworkError>)>,
    /// A channel used to send the result of sending a list of peers.
    handshake_peer_list_tx: Sender<IpAddr>,
    /// A channel used to receive the result of sending a list of peers.
    handshake_peer_list_rx: Receiver<IpAddr>,
}

pub struct NetworkWorkerChannels {
    pub controller_command_rx: Receiver<NetworkCommand>,
    pub controller_event_tx: Sender<NetworkEvent>,
    pub controller_manager_rx: Receiver<NetworkManagementCommand>,
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
        cfg: NetworkConfig,
        keypair: KeyPair,
        listener: Listener,
        establisher: Establisher,
        peer_info_db: PeerInfoDatabase,
        NetworkWorkerChannels {
            controller_command_rx,
            controller_event_tx,
            controller_manager_rx,
        }: NetworkWorkerChannels,
        version: Version,
        runtime: Runtime,
    ) -> NetworkWorker {
        let self_node_id = NodeId::new(keypair.get_public_key());

        let (conn_tx, connections_rx) =
            bounded::<(ConnectionId, HandshakeReturnType)>(cfg.handshake_manager_channel_size);
        let (handshake_tx, handshake_rx) =
            bounded::<(ConnectionId, HandshakeWorker)>(cfg.handshake_manager_channel_size);
        let handshake_manager_join_handle =
            start_handshake_manager(handshake_rx, conn_tx, runtime.handle().clone());

        let (node_result_tx, node_result_rx) = bounded::<(
            NodeId,
            Result<ConnectionClosureReason, NetworkError>,
        )>(cfg.node_result_channel_size);

        let (handshake_peer_list_tx, handshake_peer_list_rx) =
            bounded::<IpAddr>(cfg.handshake_peer_list_channel_size);

        let (node_event_tx, node_event_rx) = bounded::<NodeEvent>(cfg.node_event_channel_size);
        let max_wait_event = cfg.max_send_wait_network_event.to_duration();
        NetworkWorker {
            cfg,
            self_node_id,
            keypair,
            listener: Some(listener),
            establisher,
            peer_info_db,
            controller_command_rx,
            event: EventSender::new(controller_event_tx, node_event_tx, max_wait_event),
            controller_manager_rx,
            running_handshakes: HashSet::new(),
            handshake_peer_list_futures: Default::default(),
            node_event_rx,
            active_nodes: HashMap::new(),
            active_connections: HashMap::new(),
            version,
            runtime,
            connections_rx,
            handshake_tx,
            handshake_manager_join_handle,
            node_result_rx,
            node_result_tx,
            handshake_peer_list_tx,
            handshake_peer_list_rx,
        }
    }

    /// Runs the main loop of the network worker.
    pub fn run_loop(mut self) -> Result<(), NetworkError> {
        let mut out_connection_handle = None;
        let mut out_connection_rx = None;
        let mut cur_connection_id = ConnectionId::default();

        // Start a task to run the listener.
        let (listener_tx, listener_rx) = bounded::<(ReadHalf, WriteHalf, SocketAddr)>(1);
        let mut listener = self
            .listener
            .take()
            .expect("No listener at start of run loop.");
        let listener_handle = self.runtime.spawn(async move {
            loop {
                match listener.accept().await {
                    Ok(res) => {
                        let sender = listener_tx.clone();
                        Handle::current()
                            .spawn_blocking(move || {
                                sender
                                    .send(res)
                                    .expect("Failed to send listener message to network worker.");
                            })
                            .await
                            .expect(
                                "Failed to run task to send listener message to network worker.",
                            );
                    }
                    Err(err) => {
                        debug!("connection accept failed: {}", err);
                        massa_trace!("in_connection_failed", {"err": err.to_string()});
                        break;
                    }
                }
            }
        });

        loop {
            if out_connection_rx.is_none() {
                // Scope the channel creation to here,
                // to ensure all senders drop when the futureset is empty.
                let (out_connection_tx, rx) = bounded::<(
                    tokio::io::Result<(ReadHalf, WriteHalf)>,
                    IpAddr,
                )>(self.cfg.node_command_channel_size); // TODO: config

                out_connection_rx = Some(rx);

                // try to connect to candidate IPs
                let mut out_connecting_futures = FuturesUnordered::new();
                let candidate_ips = self.peer_info_db.get_out_connection_candidate_ips()?;
                for ip in candidate_ips {
                    debug!("starting outgoing connection attempt towards ip={}", ip);
                    massa_trace!("out_connection_attempt_start", { "ip": ip });
                    self.peer_info_db.new_out_connection_attempt(&ip)?;

                    // Run the future on the current thread.
                    // Note: this future doesn't actually await anything, consider removing async.
                    let mut connector = self.runtime.block_on(async {
                        self.establisher
                            .get_connector(self.cfg.connect_timeout)
                            .await
                    })?;

                    let addr = SocketAddr::new(ip, self.cfg.protocol_port);
                    out_connecting_futures.push(async move {
                        match connector.connect(addr).await {
                            Ok((reader, writer)) => (addr.ip(), Ok((reader, writer))),
                            Err(e) => (addr.ip(), Err(e)),
                        }
                    });
                }

                // Spawn an async task to handling out connection futures.
                let out_connection_tx_clone = out_connection_tx.clone();
                out_connection_handle = Some(self.runtime.spawn(async move {
                    while let Some((ip_addr, res)) = out_connecting_futures.next().await {
                        let out_connection_tx_clone = out_connection_tx_clone.clone();
                        Handle::current()
                            .spawn_blocking(move || {
                                out_connection_tx_clone
                                    .send((res, ip_addr))
                                    .expect("Failed to send out-connection message to network worker.");
                            })
                            .await
                            .expect("Failed to run task to send out-connection message to network worker.");
                    }
                }));
            }

            select! {
                // listen to manager commands.
                recv(self.controller_manager_rx) -> cmd => {
                    if cmd.is_err() {
                        break
                    }
                },

                // event received from a node.
                recv(self.node_event_rx) -> evt => {
                    self.on_node_event(
                        evt.map_err(|_| NetworkError::ChannelError("node event rx failed".into()))?
                    )?;
                }

                // Try peer list future finished.
                recv(self.handshake_peer_list_rx) -> msg => {
                    match msg {
                        Err(_) => {},
                        Ok(ip) => {
                           self.handshake_peer_list_futures.remove(&ip);
                        }
                    }
                }

                // Listener socket received.
                recv(listener_rx) -> msg => {
                    match msg {
                        Err(_) => {},
                        Ok((reader, writer, remote_addr)) => {
                            self.manage_in_connections(
                                reader,
                                writer,
                                remote_addr,
                                &mut cur_connection_id,
                            )?
                        }
                    }
                }

                // Active node run result.
                recv(self.node_result_rx) -> msg => {
                    // Should never panic, since the network worker keeps a sender around.
                    let (node_id, res) = msg.expect("Unexpected failure of node worker.");

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
                        .event.send(NetworkEvent::ConnectionClosed(node_id));
                    if let Some((connection_id, _, join_handle)) = self
                        .active_nodes
                        .remove(&node_id) {
                        massa_trace!("protocol channel closed", {"node_id": node_id});
                        self.connection_closed(connection_id, reason)?;
                        join_handle.join().expect("Failed to join on node worker handle.");
                    }
                }

                // incoming command.
                recv(self.controller_command_rx) -> cmd => {
                    if let Ok(cmd) = cmd {
                        self.manage_network_command(cmd)?;
                    }
                },

                // Handle finished handshakes.
                recv(self.connections_rx) -> msg => {
                    match msg {
                        Err(_) => {
                            // Handshake manager failed.
                            break
                        },
                        Ok((conn_id, outcome)) => {
                            self.on_handshake_finished(conn_id, outcome)?;
                        }
                    }
                },

                // Out-connections
                // `out_connection_rx` is always Some(rx) here.
                recv(out_connection_rx.as_ref().expect("No out_connection_rx.")) -> conn => {
                    match conn {
                        Err(_) => {
                            // Future set empty, re-try
                            out_connection_rx = None;
                            self.peer_info_db.update()?; // notify tick to peer db
                        },
                        Ok((res, ip_addr)) => {
                            self.manage_out_connections(
                                res,
                                ip_addr,
                                &mut cur_connection_id,
                            )?;
                        }
                    }
                },
            }
        }

        // Abort the out connection task.
        if let Some(handle) = out_connection_handle {
            handle.abort();
        }

        // Abort the listener task
        listener_handle.abort();

        // Drop the sole sender to, and join on, the handshake manager thread.
        drop(self.handshake_tx);
        self.handshake_manager_join_handle
            .join()
            .expect("Failed to join on the handshake manager thread at shutdown.");

        // Shutdown all the node workers.
        for (_, node_command_tx, handle) in self.active_nodes.into_values() {
            node_command_tx
                .send(NodeCommand::Close(ConnectionClosureReason::Normal))
                .expect("Failed to send close command to node worker.");
            let _ = self.node_result_rx.recv();
            handle
                .join()
                .expect("Failed to join on a node worker thread at shutdown.");
        }

        // Abort all the pending peerlist tasks
        for (_, handle) in self.handshake_peer_list_futures.drain() {
            handle.abort();
        }

        // stop peer info db
        self.peer_info_db.stop()?;

        // Will block until all tasks are finished.
        drop(self.runtime);

        Ok(())
    }

    /// Manages finished handshakes.
    /// Only used by the worker.
    ///
    /// # Arguments
    /// * `new_connection_id`: connection id of the connection that should be established here.
    /// * `outcome`: result returned by a handshake.
    fn on_handshake_finished(
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
                    self.connection_closed(new_connection_id, ConnectionClosureReason::Normal)?;
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
                        self.connection_closed(new_connection_id, ConnectionClosureReason::Normal)?;
                    }
                    // we don't have this node ID
                    hash_map::Entry::Vacant(entry) => {
                        massa_trace!("node_connected", {
                            "connection_id": new_connection_id,
                            "node_id": new_node_id
                        });

                        // Note connection alive.
                        let (ip, _) =
                            self.active_connections
                                .get(&new_connection_id)
                                .ok_or_else(|| {
                                    NetworkError::ActiveConnectionMissing(new_connection_id)
                                })?;
                        self.peer_info_db.peer_alive(ip)?;

                        // spawn node_controller_fn
                        let (node_command_tx, node_command_rx) =
                            bounded::<NodeCommand>(self.cfg.node_command_channel_size);
                        let node_event_tx_clone = self.event.clone_node_sender();
                        let cfg_copy = self.cfg.clone();
                        let runtime_handle = self.runtime.handle().clone();
                        let node_result_tx = self.node_result_tx.clone();
                        let node_fn_handle = thread::spawn(move || {
                            let res = NodeWorker::new(
                                cfg_copy,
                                new_node_id,
                                socket_reader,
                                socket_writer,
                                node_command_rx,
                                node_event_tx_clone,
                                runtime_handle,
                            )
                            .run_loop();
                            node_result_tx
                                .send((new_node_id, res))
                                .expect("Failed to send node run result back to network worker. ");
                        });
                        entry.insert((new_connection_id, node_command_tx.clone(), node_fn_handle));

                        let res = self.event.send(NetworkEvent::NewConnection(new_node_id));

                        // If we failed to send the event to protocol, close the connection.
                        if res.is_err() {
                            let res = node_command_tx
                                .send(NodeCommand::Close(ConnectionClosureReason::Normal));
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
                self.connection_closed(new_connection_id, ConnectionClosureReason::Failed)?;
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
                self.connection_closed(new_connection_id, ConnectionClosureReason::Failed)?;
            }
        };
        Ok(())
    }

    fn connection_closed(
        &mut self,
        id: ConnectionId,
        reason: ConnectionClosureReason,
    ) -> Result<(), NetworkError> {
        let (ip, is_outgoing) = self
            .active_connections
            .remove(&id)
            .ok_or_else(|| NetworkError::ActiveConnectionMissing(id))?;
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
    fn manage_network_command(&mut self, cmd: NetworkCommand) -> Result<(), NetworkError> {
        use crate::network_cmd_impl::*;
        match cmd {
            NetworkCommand::NodeBanByIps(ips) => on_node_ban_by_ips_cmd(self, ips)?,
            NetworkCommand::NodeBanByIds(ids) => on_node_ban_by_ids_cmd(self, ids)?,
            NetworkCommand::SendBlockHeader { node, header } => {
                on_send_block_header_cmd(self, node, header)?
            }
            NetworkCommand::AskForBlocks { list } => on_ask_for_block_cmd(self, list),
            NetworkCommand::SendBlockInfo { node, info } => {
                on_send_block_info_cmd(self, node, info)?
            }
            NetworkCommand::GetPeers(response_tx) => on_get_peers_cmd(self, response_tx),
            NetworkCommand::GetBootstrapPeers(response_tx) => {
                on_get_bootstrap_peers_cmd(self, response_tx)
            }
            NetworkCommand::SendOperations { node, operations } => {
                on_send_operations_cmd(self, node, operations)
            }
            NetworkCommand::SendOperationAnnouncements { to_node, batch } => {
                on_send_operation_batches_cmd(self, to_node, batch)
            }
            NetworkCommand::AskForOperations { to_node, wishlist } => {
                on_ask_for_operations_cmd(self, to_node, wishlist)
            }
            NetworkCommand::SendEndorsements { node, endorsements } => {
                on_send_endorsements_cmd(self, node, endorsements)
            }
            NetworkCommand::NodeSignMessage { msg, response_tx } => {
                on_node_sign_message_cmd(self, msg, response_tx)?
            }
            NetworkCommand::NodeUnbanByIds(ids) => on_node_unban_by_ids_cmd(self, ids)?,
            NetworkCommand::NodeUnbanByIps(ips) => on_node_unban_by_ips_cmd(self, ips)?,
            NetworkCommand::GetStats { response_tx } => on_get_stats_cmd(self, response_tx),
            NetworkCommand::Whitelist(ips) => on_whitelist_cmd(self, ips)?,
            NetworkCommand::RemoveFromWhitelist(ips) => on_remove_from_whitelist_cmd(self, ips)?,
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
    fn manage_out_connections(
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
    fn manage_in_connections(
        &mut self,
        reader: ReadHalf,
        writer: WriteHalf,
        remote_addr: SocketAddr,
        cur_connection_id: &mut ConnectionId,
    ) -> Result<(), NetworkError> {
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
        &mut self,
        reader: ReadHalf,
        writer: WriteHalf,
        remote_addr: SocketAddr,
    ) {
        massa_trace!(
            "Maximum connection count reached. Inbound connection refused, trying to send a list of peers",
            {"address": remote_addr}
        );
        if self.cfg.max_in_connection_overflow > self.handshake_peer_list_futures.len() {
            let msg = Message::PeerList(self.peer_info_db.get_advertisable_peer_ips());
            let timeout = self.cfg.peer_list_send_timeout.to_duration();
            let max_bytes_read = self.cfg.max_bytes_read;
            let max_bytes_write = self.cfg.max_bytes_write;
            let max_ask_blocks = self.cfg.max_ask_blocks;
            let max_operations_per_block = self.cfg.max_operations_per_block;
            let thread_count = self.cfg.thread_count;
            let endorsement_count = self.cfg.endorsement_count;
            let max_advertise_length = self.cfg.max_peer_advertise_length;
            let max_endorsements_per_message = self.cfg.max_endorsements_per_message;
            let max_operations_per_message = self.cfg.max_operations_per_message;
            let max_message_size = self.cfg.max_message_size;
            let max_datastore_value_length = self.cfg.max_datastore_value_length;
            let max_function_name_length = self.cfg.max_function_name_length;
            let max_parameters_size = self.cfg.max_parameters_size;
            let max_op_datastore_entry_count = self.cfg.max_op_datastore_entry_count;
            let max_op_datastore_key_length = self.cfg.max_op_datastore_key_length;
            let max_op_datastore_value_length = self.cfg.max_op_datastore_value_length;
            let sender_clone = self.handshake_peer_list_tx.clone();
            let ip = remote_addr.ip();
            self.handshake_peer_list_futures
                .insert(remote_addr.ip(), self.runtime.spawn(async move {
                    let mut writer = WriteBinder::new(writer, max_bytes_read, max_message_size);
                    let mut reader = ReadBinder::new(
                        reader,
                        max_bytes_write,
                        max_message_size,
                        MessageDeserializer::new(
                            thread_count,
                            endorsement_count,
                            max_advertise_length,
                            max_ask_blocks,
                            max_operations_per_block,
                            max_operations_per_message,
                            max_endorsements_per_message,
                            max_datastore_value_length,
                            max_function_name_length,
                            max_parameters_size,
                            max_op_datastore_entry_count,
                            max_op_datastore_key_length,
                            max_op_datastore_value_length,
                        ),
                    );
                    match tokio::time::timeout(
                        timeout,
                        futures::future::try_join(writer.send(&msg), reader.next()),
                    )
                    .await
                    {
                        Ok(Err(e)) => {
                            massa_trace!("Ignored network error when sending peer list", {
                                "error": format!("{:?}", e)
                            })
                        }
                        Err(_) => massa_trace!("Ignored timeout error when sending peer list", {}),
                        _ => (),
                    }
                    // Notify end of task to network worker.
                    Handle::current()
                        .spawn_blocking(move || {
                            let _ = sender_clone.send(ip);
                        })
                        .await
                        .expect("Failed to run task to send peer list task end notification to network worker.");
                }));
        }
    }

    /// Manage a successful incoming and outgoing connection.
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
        let worker = HandshakeWorker::new(
            reader,
            writer,
            self.self_node_id,
            self.keypair.clone(),
            self.cfg.connect_timeout,
            self.version,
            self.cfg.max_bytes_read,
            self.cfg.max_bytes_write,
        );
        if self.handshake_tx.send((connection_id, worker)).is_err() {
            return Err(NetworkError::HandshakeError(
                HandshakeErrorType::ManagementFailed,
            ));
        } else {
            debug!("starting handshake with connection_id={}", connection_id);
            massa_trace!("network_worker.new_connection", {
                "connection_id": connection_id
            });
        }
        Ok(())
    }

    /// Manages node events.
    /// Only used by the worker.
    ///
    /// # Argument
    /// * `evt`: optional node event to process.
    fn on_node_event(&mut self, evt: NodeEvent) -> Result<(), NetworkError> {
        use crate::network_event::*;
        match evt {
            // received a list of peers
            NodeEvent(from_node_id, NodeEventType::ReceivedPeerList(lst)) => {
                event_impl::on_received_peer_list(self, from_node_id, &lst)?
            }
            NodeEvent(from_node_id, NodeEventType::ReceivedAskForBlocks(list)) => {
                event_impl::on_received_ask_for_blocks(self, from_node_id, list)
            }
            NodeEvent(from_node_id, NodeEventType::ReceivedReplyForBlocks(list)) => {
                event_impl::on_received_block_info(self, from_node_id, list)?
            }
            NodeEvent(source_node_id, NodeEventType::ReceivedBlockHeader(header)) => {
                event_impl::on_received_block_header(self, source_node_id, header)?
            }
            NodeEvent(from_node_id, NodeEventType::AskedPeerList) => {
                event_impl::on_asked_peer_list(self, from_node_id)?
            }
            NodeEvent(node, NodeEventType::ReceivedOperations(operations)) => {
                event_impl::on_received_operations(self, node, operations)
            }
            NodeEvent(node, NodeEventType::ReceivedEndorsements(endorsements)) => {
                event_impl::on_received_endorsements(self, node, endorsements)
            }
            NodeEvent(node, NodeEventType::ReceivedOperationAnnouncements(operation_ids)) => {
                event_impl::on_received_operations_annoncement(self, node, operation_ids)
            }
            NodeEvent(node, NodeEventType::ReceivedAskForOperations(operation_ids)) => {
                event_impl::on_received_ask_for_operations(self, node, operation_ids)
            }
        }
        Ok(())
    }
}
