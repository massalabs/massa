use super::binders::{ReadBinder, WriteBinder};
use super::config::ProtocolConfig;
use super::messages::Message;
use crate::crypto::signature::{PrivateKey, PublicKey, SignatureEngine};
use crate::network::connection_controller::{
    ConnectionClosureReason, ConnectionController, ConnectionEvent, ConnectionId,
};
use futures::{
    future::try_join, future::FusedFuture, stream::FuturesUnordered, FutureExt, StreamExt,
};
use rand::{rngs::StdRng, FromEntropy, RngCore};
use std::collections::{hash_map, HashMap, HashSet};
use std::error::Error;
use std::net::IpAddr;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};

type BoxResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct NodeId(PublicKey);

#[derive(Clone, Debug)]
enum ProtocolCommand {}

#[derive(Clone, Debug)]
pub enum ProtocolEvent {
    ReceivedTransaction(String),
    ReceivedBlock(String),
}

#[derive(Clone, Debug)]
enum NodeCommand {
    SendPeerList(Vec<IpAddr>),
    SendBlock(String),
    SendTransaction(String),
    Close,
}

#[derive(Clone, Debug)]
enum NodeEventType {
    AskedPeerList,
    ReceivedPeerList(Vec<IpAddr>),
    ReceivedBlock(String),       //put some date for the example
    ReceivedTransaction(String), //put some date for the example
    Closed(ConnectionClosureReason),
}

#[derive(Clone, Debug)]
struct NodeEvent(NodeId, NodeEventType);

pub struct ProtocolController {
    protocol_event_rx: Receiver<ProtocolEvent>,
    protocol_command_tx: Sender<ProtocolCommand>,
    protocol_controller_handle: JoinHandle<()>,
}

impl ProtocolController {
    pub async fn new(cfg: &ProtocolConfig) -> BoxResult<Self> {
        let connection_controller = ConnectionController::new(&cfg.network).await?;

        //generate our own random PublicKey (and therefore NodeId) and keep private key
        let private_key;
        let self_node_id;
        {
            let signature_engine = SignatureEngine::new();
            let mut rng = StdRng::from_entropy();
            private_key = SignatureEngine::generate_random_private_key(&mut rng);
            self_node_id = NodeId(signature_engine.derive_public_key(&private_key));
        }

        // launch worker
        let (protocol_event_tx, protocol_event_rx) = mpsc::channel::<ProtocolEvent>(1024);
        let (protocol_command_tx, protocol_command_rx) = mpsc::channel::<ProtocolCommand>(1024);
        let cfg_copy = cfg.clone();
        let protocol_controller_handle = tokio::spawn(async move {
            protocol_controller_fn(
                cfg_copy,
                self_node_id,
                private_key,
                connection_controller,
                protocol_event_tx,
                protocol_command_rx,
            )
            .await;
        });

        Ok(ProtocolController {
            protocol_event_rx,
            protocol_command_tx,
            protocol_controller_handle,
        })
    }

    //Receives the next ProtocolEvent from connected Node.
    //None is returned when all Sender halves have dropped, indicating that no further values can be sent on the channel
    pub async fn wait_event(&mut self) -> ProtocolEvent {
        self.protocol_event_rx
            .recv()
            .await
            .expect("failed retrieving protocol controller event")
    }

    //stop the protocol controller
    pub async fn stop(mut self) {
        drop(self.protocol_command_tx);
        while let Some(_) = self.protocol_event_rx.next().await {}
        self.protocol_controller_handle
            .await
            .expect("failed joining protocol controller");
    }
}

async fn protocol_controller_fn(
    cfg: ProtocolConfig,
    self_node_id: NodeId,
    private_key: PrivateKey,
    mut connection_controller: ConnectionController,
    controller_event_tx: Sender<ProtocolEvent>,
    mut controller_command_rx: Receiver<ProtocolCommand>,
) {
    let mut running_handshakes: HashSet<ConnectionId> = HashSet::new(); // number of running handshakes for each connection ID
    let mut handshake_futures = FuturesUnordered::new(); //list of currently running handshake response futures (see fn_handshake)

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

            // listen to connection controller event
            evt = connection_controller.wait_event() => match evt {
                ConnectionEvent::NewConnection((connection_id, socket)) => {
                    // add connection ID to running_handshakes
                    // launch async fn_handshake(connectionId, socket)
                    // add its handle to handshake_futures
                    if !running_handshakes.insert(connection_id) {
                        panic!("expect that the id is not already in running_handshakes");
                    }
                    let messsage_timeout_copy = Duration::from_secs_f32(cfg.message_timeout_seconds);
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
                ConnectionEvent::ConnectionBanned(connection_id) => {
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
            },


            // wait for a handshake future to complete
            res = handshake_futures.next() => match res {
                // a handshake finished, and succeeded
                Some(Ok((new_connection_id, Ok((new_node_id, socket_reader, socket_writer))))) =>  {
                    // connection was banned in the meantime
                    if !running_handshakes.remove(&new_connection_id) {
                        connection_controller
                            .connection_closed(new_connection_id, ConnectionClosureReason::Normal)
                            .await;
                        continue;
                    }

                    match active_nodes.entry(new_node_id) {
                        // we already have this node ID
                        hash_map::Entry::Occupied(_) => {
                            connection_controller
                                .connection_closed(new_connection_id, ConnectionClosureReason::Normal)
                                .await;
                        },
                        // we don't have this node ID
                        hash_map::Entry::Vacant(entry) => {
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
                                    node_event_tx_clone
                                )
                                .await
                            });
                            entry.insert((new_connection_id, node_command_tx, node_fn_handle));
                        }
                    }
                },
                // a handshake finished and failed
                Some(Ok((connection_id, Err(err)))) => {
                    log::error!("handshake:{:?} return an error:{:?}", connection_id, err);
                    running_handshakes.remove(&connection_id);
                    connection_controller.connection_closed(connection_id, ConnectionClosureReason::Failed).await;
                },
                Some(Err(err)) => panic!("running handshake future await returned an error:{:?}", err),
                None => (),
            },

            // event received from a node
            Some(NodeEvent(from_node_id, event)) = node_event_rx.next() =>  match event {
                // received a list of peers
                NodeEventType::ReceivedPeerList(lst) => {
                    let (connection_id, _, _) = active_nodes.get(&from_node_id).expect("event from msising node");
                    connection_controller.connection_alive(*connection_id).await;
                    connection_controller.merge_advertised_peer_list(lst).await;
                }
                // received block (TODO test only)
                NodeEventType::ReceivedBlock(data) => controller_event_tx
                    .send(ProtocolEvent::ReceivedBlock(data))
                    .await
                    .expect("controller event tx failed"),
                // received transaction (TODO test only)
                NodeEventType::ReceivedTransaction(data) => controller_event_tx
                    .send(ProtocolEvent::ReceivedTransaction(data))
                    .await
                    .expect("controller event tx failed"),
                // connection closed
                NodeEventType::Closed(reason) => {
                    let (connection_id, _, handle) = active_nodes.remove(&from_node_id).expect("event from msising node");
                    connection_controller.connection_closed(connection_id, reason).await;
                    handle.await.expect("could not join node handle");
                },
                // asked peer list
                NodeEventType::AskedPeerList => {
                    let (_, node_command_tx, _) = active_nodes.get(&from_node_id).expect("event received from missing node");
                    node_command_tx
                        .send(NodeCommand::SendPeerList(connection_controller.get_advertisable_peer_list().await))
                        .await.expect("controller event tx failed");
                }
            }

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

    // stop connection controller
    connection_controller.stop().await;
}

async fn handshake_fn(
    connection_id: ConnectionId,
    socket: TcpStream,
    self_node_id: NodeId, // NodeId.0 is our PublicKey
    private_key: PrivateKey,
    timeout_duration: Duration,
) -> (
    ConnectionId,
    Result<
        (
            NodeId,
            ReadBinder<OwnedReadHalf>,
            WriteBinder<OwnedWriteHalf>,
        ),
        String,
    >,
) {
    // split socket, bind reader and writer
    let (socket_reader, socket_writer) = socket.into_split();
    let (mut reader, mut writer) = (
        ReadBinder::new(socket_reader),
        WriteBinder::new(socket_writer),
    );

    // generate random bytes
    let mut self_random_bytes = vec![0u8; 64];
    StdRng::from_entropy().fill_bytes(&mut self_random_bytes);

    // send handshake init future
    let send_init_msg = Message::HandshakeInitiation {
        public_key: self_node_id.0,
        random_bytes: self_random_bytes.clone(),
    };
    let send_init_fut = writer.send(&send_init_msg);

    // receive handshake init future
    let recv_init_fut = reader.next();

    // join send_init_fut and recv_init_fut with a timeout, and match result
    let (other_node_id, other_random_bytes) =
        match timeout(timeout_duration, try_join(send_init_fut, recv_init_fut)).await {
            Err(_) => return (connection_id, Err("handshake init timed out".into())),
            Ok(Err(_)) => return (connection_id, Err("handshake init r/w failed".into())),
            Ok(Ok((_, None))) => return (connection_id, Err("handshake init stopped".into())),
            Ok(Ok((_, Some((_, msg))))) => match msg {
                Message::HandshakeInitiation {
                    public_key: pk,
                    random_bytes: rb,
                } => (NodeId(pk), rb),
                _ => return (connection_id, Err("handshake init wrong message".into())),
            },
        };

    // check if remote node ID is the same as ours
    if other_node_id == self_node_id {
        return (
            connection_id,
            Err("handshake announced own public key".into()),
        );
    }

    // sign their random bytes
    let signature_engine = SignatureEngine::new();
    let self_signature = signature_engine.sign(&other_random_bytes, &private_key);

    // send handshake reply future
    let send_reply_msg = Message::HandshakeReply {
        signature: self_signature,
    };
    let send_reply_fut = writer.send(&send_reply_msg);

    // receive handshake reply future
    let recv_reply_fut = reader.next();

    // join send_reply_fut and recv_reply_fut with a timeout, and match result
    let other_signature =
        match timeout(timeout_duration, try_join(send_reply_fut, recv_reply_fut)).await {
            Err(_) => return (connection_id, Err("handshake reply timed out".into())),
            Ok(Err(_)) => return (connection_id, Err("handshake reply r/w failed".into())),
            Ok(Ok((_, None))) => return (connection_id, Err("handshake reply stopped".into())),
            Ok(Ok((_, Some((_, msg))))) => match msg {
                Message::HandshakeReply { signature: sig } => sig,
                _ => return (connection_id, Err("handshake reply wrong message".into())),
            },
        };

    // check their signature
    if !signature_engine.verify(&self_random_bytes, &other_signature, &other_node_id.0) {
        return (
            connection_id,
            Err("invalid remote handshake signature".into()),
        );
    }

    (connection_id, Ok((other_node_id, reader, writer)))
}

async fn node_controller_fn(
    cfg: ProtocolConfig,
    node_id: NodeId,
    mut socket_reader: ReadBinder<OwnedReadHalf>,
    socket_writer: WriteBinder<OwnedWriteHalf>,
    mut node_command_rx: Receiver<NodeCommand>,
    node_event_tx: Sender<NodeEvent>,
) {
    let (writer_command_tx, writer_command_rx) = mpsc::channel::<Message>(1024);
    let (writer_event_tx, writer_event_rx) = tokio::sync::oneshot::channel::<bool>(); // true = OK, false = ERROR
    let mut fused_writer_event_rx = writer_event_rx.fuse();

    let cfg_copy = cfg.clone();
    let node_writer_handle = tokio::spawn(async move {
        node_writer_fn(cfg_copy, socket_writer, writer_event_tx, writer_command_rx).await;
    });

    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs_f32(
        cfg.ask_peer_list_interval_seconds,
    ));

    let mut exit_reason = ConnectionClosureReason::Normal;

    loop {
        tokio::select! {
            // incoming socket data
            res = socket_reader.next() => match res {
                Ok(Some((_, msg))) => {
                    match msg {
                        Message::Block(block) => node_event_tx.send(
                                NodeEvent(node_id, NodeEventType::ReceivedBlock(block))
                            ).await.expect("node_event_tx died"),
                        Message::Transaction(tr) =>  node_event_tx.send(
                                NodeEvent(node_id, NodeEventType::ReceivedTransaction(tr))
                            ).await.expect("node_event_tx died"),
                        Message::PeerList(pl) =>  node_event_tx.send(
                                NodeEvent(node_id, NodeEventType::ReceivedPeerList(pl))
                            ).await.expect("node_event_tx died"),
                        Message::AskPeerList => node_event_tx.send(
                                NodeEvent(node_id, NodeEventType::AskedPeerList)
                            ).await.expect("node_event_tx died"),
                        _ => {  // wrong message
                            exit_reason = ConnectionClosureReason::Failed;
                            break;
                        },
                    }
                },
                Ok(None)=> break, // peer closed cleanly
                Err(_) => {  //stream error
                    exit_reason = ConnectionClosureReason::Failed;
                    break;
                },
            },

            // node command
            cmd = node_command_rx.next() => match cmd {
                Some(NodeCommand::Close) => break,
                Some(NodeCommand::SendPeerList(ip_vec)) => {
                    writer_command_tx.send(Message::PeerList(ip_vec)).await.expect("writer disappeared");
                }
                Some(NodeCommand::SendBlock(block)) => {
                    writer_command_tx.send(Message::Block(block)).await.expect("writer disappeared");
                }
                Some(NodeCommand::SendTransaction(transaction)) => {
                    writer_command_tx.send(Message::Transaction(transaction)).await.expect("writer disappeared");
                }
                /*Some(_) => {
                    panic!("unknown protocol command")
                },*/
                None => {
                    panic!("the protocol controller should have close everything before shuting down")
                },
            },

            // writer event
            evt = &mut fused_writer_event_rx => {
                if !evt.expect("writer_evt_rx died") {
                    exit_reason = ConnectionClosureReason::Failed;
                }
                break;
            },

            _ = interval.tick() => {
                writer_command_tx.send(Message::AskPeerList).await.expect("writer disappeared");
            }
        }
    }

    // close writer
    drop(writer_command_tx);
    if !fused_writer_event_rx.is_terminated() {
        fused_writer_event_rx
            .await
            .expect("writer_evt_tx already closed");
    }
    node_writer_handle
        .await
        .expect("node_writer_handle already closed");

    // notify protocol controller of closure
    node_event_tx
        .send(NodeEvent(node_id, NodeEventType::Closed(exit_reason)))
        .await
        .expect("node_event_tx already closed");
}

async fn node_writer_fn(
    cfg: ProtocolConfig,
    mut socket_writer: WriteBinder<OwnedWriteHalf>,
    writer_event_tx: tokio::sync::oneshot::Sender<bool>,
    mut writer_command_rx: Receiver<Message>,
) {
    let write_timeout = Duration::from_secs_f32(cfg.message_timeout_seconds);
    let mut clean_exit = true;
    loop {
        match writer_command_rx.next().await {
            Some(msg) => {
                if let Err(_) = timeout(write_timeout, socket_writer.send(&msg)).await {
                    clean_exit = false;
                    break;
                }
            }
            None => break,
        }
    }
    writer_event_tx
        .send(clean_exit)
        .expect("writer_evt_tx died");
}
