use super::super::{
    config::ProtocolConfig,
    handshake_worker::{HandshakeReturnType, HandshakeWorker},
    protocol_controller::*,
};
use super::node_worker::{NodeCommand, NodeEvent, NodeEventType, NodeWorker};
use crate::error::{ChannelError, CommunicationError, HandshakeErrorType};
use crate::network::network_controller::{
    ConnectionClosureReason, ConnectionId, NetworkController, NetworkEvent,
};
use crate::network::PeerInfo;
use crypto::{hash::Hash, signature::PrivateKey};
use futures::{stream::FuturesUnordered, StreamExt};
use models::block::Block;
use std::{
    collections::{hash_map, HashMap, HashSet},
    net::IpAddr,
};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

/// Commands that protocol worker can process
#[derive(Debug)]
pub enum ProtocolCommand {
    /// Propagate given block
    PropagateBlock { hash: Hash, block: Block },
    /// Get all known peers
    GetPeers(oneshot::Sender<HashMap<IpAddr, PeerInfo>>),
}

pub struct ProtocolWorker<NetworkControllerT: 'static + NetworkController> {
    /// Protocol configuration.
    cfg: ProtocolConfig,
    /// Our node id.
    self_node_id: NodeId,
    /// Our private key.
    private_key: PrivateKey,
    /// Associated nework controller.
    network_controller: NetworkControllerT,
    /// Channel to send protocol events to the controller.
    controller_event_tx: Sender<ProtocolEvent>,
    /// Channel receiving commands from the controller.
    controller_command_rx: Receiver<ProtocolCommand>,
    /// Set of connection id of node with running handshake.
    running_handshakes: HashSet<ConnectionId>,
    /// Running handshakes futures.
    handshake_futures:
        FuturesUnordered<JoinHandle<(ConnectionId, HandshakeReturnType<NetworkControllerT>)>>,
    /// Ids of active nodes mapped to Connection id, node command sender and handle on the associated node worker.
    active_nodes: HashMap<NodeId, (ConnectionId, mpsc::Sender<NodeCommand>, JoinHandle<()>)>,
    /// Optional channel for sending node events.
    node_event_tx_opt: Option<Sender<NodeEvent>>,
    /// Receiving channel for node events.
    node_event_rx: Receiver<NodeEvent>,
}

impl<NetworkControllerT: 'static + NetworkController> ProtocolWorker<NetworkControllerT> {
    /// Creates a new protocol worker.
    ///
    /// # Arguments
    /// * cfg: protocol configuration.
    /// * self_node_id: our private key.
    /// * network_controller associated network controller.
    /// * controller_event_tx: Channel to send protocol events to the controller.
    /// * controller_command_rx: Channel receiving commands from the controller.
    pub fn new(
        cfg: ProtocolConfig,
        self_node_id: NodeId,
        private_key: PrivateKey,
        network_controller: NetworkControllerT,
        controller_event_tx: Sender<ProtocolEvent>,
        controller_command_rx: Receiver<ProtocolCommand>,
    ) -> ProtocolWorker<NetworkControllerT> {
        let (node_event_tx, node_event_rx) = mpsc::channel::<NodeEvent>(1024);
        ProtocolWorker {
            cfg,
            self_node_id,
            private_key,
            network_controller,
            controller_event_tx,
            controller_command_rx,
            running_handshakes: HashSet::new(),
            handshake_futures: FuturesUnordered::new(),
            active_nodes: HashMap::new(),
            node_event_tx_opt: Some(node_event_tx),
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
    pub async fn run_loop(mut self) -> Result<(), CommunicationError> {
        loop {
            tokio::select! {
                // listen to incoming commands
                res = self.controller_command_rx.recv() => match res {
                    Some(ProtocolCommand::PropagateBlock {
                        hash,
                        block,
                    }) => {
                        massa_trace!("block_propagation", {"block": hash});
                        // TODO in the future, send only to the ones that don't already have it (see issue #94)
                        for (_, (_, node_command_tx, _)) in self.active_nodes.iter() {
                            node_command_tx
                                .send(NodeCommand::SendBlock(block.clone()))
                                .await
                                .map_err(|err| ChannelError::from(err))?;
                        }
                    }
                    Some(ProtocolCommand::GetPeers(response_tx)) => {
                        response_tx.send(self
                            .network_controller
                            .get_peers()
                            .await
                            .map_err(|_| ChannelError::DatabaseCommunicationError)?
                        ).map_err(|_| ChannelError::DatabaseCommunicationError)?;
                    }
                    None => break  // finished
                },

                // listen to network controller event
                evt = self.network_controller.wait_event() => self.on_network_event(evt?).await?,

                // wait for a handshake future to complete
                Some(res) = self.handshake_futures.next() => {
                    let (conn_id, outcome) = res?;
                    self.on_handshake_finished(conn_id, outcome).await?;
                },

                // event received from a node
                evt = self.node_event_rx.recv() => self.on_node_event(evt).await?,
            } //end select!
        } //end loop

        // Cleanup

        {
            // close all active nodes
            let mut node_handle_set = FuturesUnordered::new();
            // drop senders
            let _ = self.node_event_tx_opt.take();
            for (_, (_, _, handle)) in self.active_nodes.drain() {
                node_handle_set.push(handle);
            }
            while let Some(_) = self.node_event_rx.recv().await {}
            while let Some(_) = node_handle_set.next().await {}
        }

        // wait for all running handshakes
        self.running_handshakes.clear();
        while let Some(_) = self.handshake_futures.next().await {}

        // stop network controller
        self.network_controller.stop().await?;

        Ok(())
    }

    /// Manages network event
    /// Only used by the worker.
    ///
    /// # Argument
    /// evt: event to processs
    async fn on_network_event(
        &mut self,
        evt: NetworkEvent<NetworkControllerT::ReaderT, NetworkControllerT::WriterT>,
    ) -> Result<(), CommunicationError> {
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
                        HandshakeWorker::<NetworkControllerT>::new(
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
                for (c_id, node_tx, _) in self.active_nodes.values() {
                    if *c_id == connection_id {
                        node_tx
                            .send(NodeCommand::Close)
                            .await
                            .map_err(|err| ChannelError::from(err))?;
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
        outcome: HandshakeReturnType<NetworkControllerT>,
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
                    self.network_controller
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
                        self.network_controller
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
                        self.network_controller
                            .connection_alive(new_connection_id)
                            .await?;
                        // spawn node_controller_fn
                        let (node_command_tx, node_command_rx) = mpsc::channel::<NodeCommand>(1024);
                        let node_event_tx_clone = self
                            .node_event_tx_opt
                            .as_ref()
                            .ok_or(ChannelError::NodeEventSenderError)?
                            .clone();
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
                            .expect("NodeWorker loop crash") //in a spawned task
                        });
                        entry.insert((new_connection_id, node_command_tx, node_fn_handle));
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
                self.network_controller
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
    async fn on_node_event(&mut self, evt: Option<NodeEvent>) -> Result<(), CommunicationError> {
        match evt {
            // received a list of peers
            Some(NodeEvent(from_node_id, NodeEventType::ReceivedPeerList(lst))) => {
                debug!("node_id={:?} sent us a peer list: {:?}", from_node_id, lst);
                massa_trace!("peer_list_received", {
                    "node_id": from_node_id,
                    "ips": lst
                });
                let (connection_id, _, _) = self
                    .active_nodes
                    .get(&from_node_id)
                    .ok_or(CommunicationError::MissingNodeError)?;
                self.network_controller
                    .connection_alive(*connection_id)
                    .await?;
                self.network_controller
                    .merge_advertised_peer_list(lst)
                    .await?;
            }
            Some(NodeEvent(from_node_id, NodeEventType::ReceivedBlock(data))) => self
                .controller_event_tx
                .send(ProtocolEvent(
                    from_node_id,
                    ProtocolEventType::ReceivedBlock(data),
                ))
                .await
                .map_err(|err| ChannelError::from(err))?,
            Some(NodeEvent(from_node_id, NodeEventType::ReceivedTransaction(data))) => self
                .controller_event_tx
                .send(ProtocolEvent(
                    from_node_id,
                    ProtocolEventType::ReceivedTransaction(data),
                ))
                .await
                .map_err(|err| ChannelError::from(err))?,
            // connection closed
            Some(NodeEvent(from_node_id, NodeEventType::Closed(reason))) => {
                let (connection_id, _, handle) = self
                    .active_nodes
                    .remove(&from_node_id)
                    .ok_or(CommunicationError::MissingNodeError)?;
                info!("protocol channel closed peer_id={:?}", from_node_id);
                massa_trace!("node_closed", {
                    "node_id": from_node_id,
                    "reason": reason
                });
                self.network_controller
                    .connection_closed(connection_id, reason)
                    .await?;
                handle.await?;
            }
            // asked peer list
            Some(NodeEvent(from_node_id, NodeEventType::AskedPeerList)) => {
                debug!("node_id={:?} asked us for peer list", from_node_id);
                massa_trace!("node_asked_peer_list", { "node_id": from_node_id });
                let (_, node_command_tx, _) = self
                    .active_nodes
                    .get(&from_node_id)
                    .ok_or(CommunicationError::MissingNodeError)?;
                node_command_tx
                    .send(NodeCommand::SendPeerList(
                        self.network_controller.get_advertisable_peer_list().await?,
                    ))
                    .await
                    .map_err(|err| ChannelError::from(err))?;
            }
            None => return Err(ChannelError::NodeEventReceiverError.into()),
        }
        Ok(())
    }
}
