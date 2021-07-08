use super::config::ProtocolConfig;
use super::handshake::{handshake_fn, HandshakeReturnType};
use super::node_controller::node_controller_fn;
use super::node_controller::{NodeCommand, NodeEvent, NodeEventType, NodeId};
use crate::crypto::hash::Hash;
use crate::crypto::signature::{PrivateKey, SignatureEngine};
use crate::logging::{debug, info, trace};
use crate::network::network_controller::{
    ConnectionClosureReason, ConnectionId, NetworkController, NetworkEvent,
};
use crate::structures::block::Block;
use futures::{stream::FuturesUnordered, StreamExt};
use rand::{rngs::StdRng, FromEntropy};
use std::collections::{hash_map, HashMap, HashSet};
use std::error::Error;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;

type BoxResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Debug)]
enum ProtocolCommand {
    PropagateBlock {
        restrict_to_node: Option<NodeId>,
        exclude_node: Option<NodeId>,
        block: Block,
    },
}

#[derive(Clone, Debug)]
pub enum ProtocolEventType {
    ReceivedTransaction(String),
    ReceivedBlock(Block),
    AskedBlock(Hash),
}

#[derive(Clone, Debug)]
pub struct ProtocolEvent(pub NodeId, pub ProtocolEventType);

pub struct ProtocolController {
    protocol_event_rx: Receiver<ProtocolEvent>,
    protocol_command_tx: Sender<ProtocolCommand>,
    protocol_controller_handle: JoinHandle<()>,
}

impl ProtocolController {
    /// initiate a new ProtocolController from a ProtocolConfig
    /// - generate public / private key
    /// - create protocol_command/protocol_event channels
    /// - lauch protocol_controller_fn in an other task
    /// returns the ProtocolController in a BoxResult once it is ready
    pub async fn new(
        cfg: &ProtocolConfig,
        network_controller: NetworkController,
    ) -> BoxResult<Self> {
        debug!("starting protocol controller");
        massa_trace!("start", {});

        // generate our own random PublicKey (and therefore NodeId) and keep private key
        let private_key;
        let self_node_id;
        {
            let signature_engine = SignatureEngine::new();
            let mut rng = StdRng::from_entropy();
            private_key = SignatureEngine::generate_random_private_key(&mut rng);
            self_node_id = NodeId(signature_engine.derive_public_key(&private_key));
        }
        debug!("local protocol node_id={:?}", self_node_id);
        massa_trace!("self_node_id", { "node_id": self_node_id });

        // launch worker
        let (protocol_event_tx, protocol_event_rx) = mpsc::channel::<ProtocolEvent>(1024);
        let (protocol_command_tx, protocol_command_rx) = mpsc::channel::<ProtocolCommand>(1024);
        let cfg_copy = cfg.clone();
        let protocol_controller_handle = tokio::spawn(async move {
            protocol_controller_fn(
                cfg_copy,
                self_node_id,
                private_key,
                network_controller,
                protocol_event_tx,
                protocol_command_rx,
            )
            .await;
        });

        debug!("protocol controller ready");
        massa_trace!("ready", {});

        Ok(ProtocolController {
            protocol_event_rx,
            protocol_command_tx,
            protocol_controller_handle,
        })
    }

    /// Receives the next ProtocolEvent from connected Node.
    /// None is returned when all Sender halves have dropped,
    /// indicating that no further values can be sent on the channel
    /// panics if the protocol controller event can't be retrieved
    pub async fn wait_event(&mut self) -> ProtocolEvent {
        self.protocol_event_rx
            .recv()
            .await
            .expect("failed retrieving protocol controller event")
    }

    /// Stop the protocol controller
    /// panices id the protocol controller is not reachable
    pub async fn stop(mut self) {
        debug!("stopping protocol controller");
        massa_trace!("begin", {});
        drop(self.protocol_command_tx);
        while let Some(_) = self.protocol_event_rx.next().await {}
        self.protocol_controller_handle
            .await
            .expect("failed joining protocol controller");
        debug!("protocol controller stopped");
        massa_trace!("end", {});
    }

    fn propagate_block(
        &mut self,
        block: Block,
        excluding: Option<Vec<NodeId>>,
        includin: Option<Vec<NodeId>>,
    ) {
        unimplemented!()
    }
}

/// this function is launched in a separated task
/// It is the link between the ProtocolController and node_controller_fn
/// It is mostly a tokio::select inside a loop
/// wainting on :
/// - controller_command_rx
/// - network_controller
/// - handshake_futures
/// - node_event_rx
/// And at the end every thing is close properly
///
/// panics if :
/// - a new node_id is already in running_handshakes while it shouldn't
/// - node command tx disappeared
/// - running handshake future await returned an error
/// - event from misising node through node_event_rx
/// - controller event tx failed
/// - node_event_rx died
async fn protocol_controller_fn(
    cfg: ProtocolConfig,
    self_node_id: NodeId,
    private_key: PrivateKey,
    mut network_controller: NetworkController,
    controller_event_tx: Sender<ProtocolEvent>,
    mut controller_command_rx: Receiver<ProtocolCommand>,
) {
    // number of running handshakes for each connection ID
    let mut running_handshakes: HashSet<ConnectionId> = HashSet::new();

    //list of currently running handshake response futures (see handshake_fn)
    let mut handshake_futures: FuturesUnordered<JoinHandle<HandshakeReturnType>> =
        FuturesUnordered::new();

    // list of active nodes we are connected to
    let mut active_nodes: HashMap<
        NodeId,
        (ConnectionId, mpsc::Sender<NodeCommand>, JoinHandle<()>),
    > = HashMap::new();

    // define node_event MPSC for events coming from nodes
    let (node_event_tx, mut node_event_rx) = mpsc::channel::<NodeEvent>(1024);

    loop {
        tokio::select! {
            // listen to incoming commands
            res = controller_command_rx.next() => match res {
                Some(_) => {}  // TODO
                None => break  // finished
            },

            // listen to network controller event
            evt = network_controller.wait_event() => manage_network_event(
                evt,
                &mut running_handshakes,
                &cfg,
                self_node_id,
                private_key,
                &mut handshake_futures,
                &active_nodes
            ).await,


            // wait for a handshake future to complete
            Some(res) = handshake_futures.next() => manage_handshake_futures(
                res,
                &mut running_handshakes,
                &mut active_nodes,
                &mut network_controller,
                &node_event_tx,
                &cfg,
            ).await,

            // event received from a node
            event = node_event_rx.next() => manage_node_event(
                event,
                &mut active_nodes,
                &mut network_controller,
                &controller_event_tx,
            ).await,

        } //end select!
    } //end loop

    {
        // close all active nodes
        let mut node_handle_set = FuturesUnordered::new();
        // drop senders
        for (_, (_, _, handle)) in active_nodes.drain() {
            node_handle_set.push(handle);
        }
        while let Some(_) = node_event_rx.next().await {}
        while let Some(_) = node_handle_set.next().await {}
    }

    // wait for all running handshakes
    running_handshakes.clear();
    while let Some(_) = handshake_futures.next().await {}

    // stop network controller
    network_controller.stop().await;
}

async fn manage_network_event(
    evt: NetworkEvent,
    running_handshakes: &mut HashSet<ConnectionId>,
    cfg: &ProtocolConfig,
    self_node_id: NodeId,
    private_key: PrivateKey,
    handshake_futures: &mut FuturesUnordered<JoinHandle<HandshakeReturnType>>,
    active_nodes: &HashMap<NodeId, (ConnectionId, mpsc::Sender<NodeCommand>, JoinHandle<()>)>,
) {
    match evt {
        NetworkEvent::NewConnection((connection_id, socket)) => {
            // add connection ID to running_handshakes
            // launch async handshake_fn(connectionId, socket)
            // add its handle to handshake_futures
            if !running_handshakes.insert(connection_id) {
                panic!("expect that the id is not already in running_handshakes");
            }

            debug!("starting handshake with connection_id={:?}", connection_id);
            massa_trace!("handshake_start", { "connection_id": connection_id });

            let messsage_timeout_copy = cfg.message_timeout;
            let handshake_fn_handle = tokio::spawn(async move {
                handshake_fn(
                    connection_id,
                    socket,
                    self_node_id,
                    private_key,
                    messsage_timeout_copy,
                )
                .await
            });
            handshake_futures.push(handshake_fn_handle);
        }
        NetworkEvent::ConnectionBanned(connection_id) => {
            // connection_banned(connectionId)
            // remove the connectionId entry in running_handshakes
            running_handshakes.remove(&connection_id);
            // find all active_node with this ConnectionId and send a NodeMessage::Close
            for (c_id, node_tx, _) in active_nodes.values() {
                if *c_id == connection_id {
                    node_tx
                        .send(NodeCommand::Close)
                        .await
                        .expect("node command tx disappeared");
                }
            }
        }
    }
}

async fn manage_handshake_futures(
    res: Result<HandshakeReturnType, tokio::task::JoinError>,
    running_handshakes: &mut HashSet<ConnectionId>,
    active_nodes: &mut HashMap<NodeId, (ConnectionId, mpsc::Sender<NodeCommand>, JoinHandle<()>)>,
    network_controller: &mut NetworkController,
    node_event_tx: &Sender<NodeEvent>,
    cfg: &ProtocolConfig,
) {
    match res {
        // a handshake finished, and succeeded
        Ok((new_connection_id, Ok((new_node_id, socket_reader, socket_writer)))) => {
            debug!(
                "handshake with connection_id={:?} succeeded => node_id={:?}",
                new_connection_id, new_node_id
            );
            massa_trace!("handshake_ok", {
                "connection_id": new_connection_id,
                "node_id": new_node_id
            });

            // connection was banned in the meantime
            if !running_handshakes.remove(&new_connection_id) {
                debug!(
                    "connection_id={:?}, node_id={:?} peer was banned while handshaking",
                    new_connection_id, new_node_id
                );
                massa_trace!("handshake_banned", {
                    "connection_id": new_connection_id,
                    "node_id": new_node_id
                });
                network_controller
                    .connection_closed(new_connection_id, ConnectionClosureReason::Normal)
                    .await;
                return;
            }

            match active_nodes.entry(new_node_id) {
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
                    network_controller
                        .connection_closed(new_connection_id, ConnectionClosureReason::Normal)
                        .await;
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
                    // spawn node_controller_fn
                    let (node_command_tx, node_command_rx) = mpsc::channel::<NodeCommand>(1024);
                    let node_event_tx_clone = node_event_tx.clone();
                    let cfg_copy = cfg.clone();
                    let node_fn_handle = tokio::spawn(async move {
                        node_controller_fn(
                            cfg_copy,
                            new_node_id,
                            socket_reader,
                            socket_writer,
                            node_command_rx,
                            node_event_tx_clone,
                        )
                        .await
                    });
                    entry.insert((new_connection_id, node_command_tx, node_fn_handle));
                }
            }
        }
        // a handshake finished and failed
        Ok((connection_id, Err(err))) => {
            debug!(
                "handshake failed with connection_id={:?}: {:?}",
                connection_id, err
            );
            massa_trace!("handshake_failed", {
                "connection_id": connection_id,
                "err": err.to_string()
            });
            running_handshakes.remove(&connection_id);
            network_controller
                .connection_closed(connection_id, ConnectionClosureReason::Failed)
                .await;
        }
        Err(err) => panic!("running handshake future await returned an error:{:?}", err),
    }
}

async fn manage_node_event(
    event: Option<NodeEvent>,
    active_nodes: &mut HashMap<NodeId, (ConnectionId, mpsc::Sender<NodeCommand>, JoinHandle<()>)>,
    network_controller: &mut NetworkController,
    controller_event_tx: &Sender<ProtocolEvent>,
) {
    match event {
        // received a list of peers
        Some(NodeEvent(from_node_id, NodeEventType::ReceivedPeerList(lst))) => {
            debug!("node_id={:?} sent us a peer list: {:?}", from_node_id, lst);
            massa_trace!("peer_list_received", {
                "node_id": from_node_id,
                "ips": lst
            });
            let (connection_id, _, _) = active_nodes
                .get(&from_node_id)
                .expect("event from missing node");
            network_controller.connection_alive(*connection_id).await;
            network_controller.merge_advertised_peer_list(lst).await;
        }
        // received block (TODO test only)
        Some(NodeEvent(from_node_id, NodeEventType::ReceivedBlock(data))) => controller_event_tx
            .send(ProtocolEvent(
                from_node_id,
                ProtocolEventType::ReceivedBlock(data),
            ))
            .await
            .expect("controller event tx failed"),
        // received transaction (TODO test only)
        Some(NodeEvent(from_node_id, NodeEventType::ReceivedTransaction(data))) => {
            controller_event_tx
                .send(ProtocolEvent(
                    from_node_id,
                    ProtocolEventType::ReceivedTransaction(data),
                ))
                .await
                .expect("controller event tx failed")
        }
        // connection closed
        Some(NodeEvent(from_node_id, NodeEventType::Closed(reason))) => {
            let (connection_id, _, handle) = active_nodes
                .remove(&from_node_id)
                .expect("event from misising node");
            info!("protocol channel closed peer_id={:?}", from_node_id);
            massa_trace!("node_closed", {
                "node_id": from_node_id,
                "reason": reason
            });
            network_controller
                .connection_closed(connection_id, reason)
                .await;
            handle.await.expect("controller event tx failed");
        }
        // asked peer list
        Some(NodeEvent(from_node_id, NodeEventType::AskedPeerList)) => {
            debug!("node_id={:?} asked us for peer list", from_node_id);
            massa_trace!("node_asked_peer_list", { "node_id": from_node_id });
            let (_, node_command_tx, _) = active_nodes
                .get(&from_node_id)
                .expect("event received from missing node");
            node_command_tx
                .send(NodeCommand::SendPeerList(
                    network_controller.get_advertisable_peer_list().await,
                ))
                .await
                .expect("controller event tx failed");
        }
        None => panic!("node_event_rx died"),
    }
}
