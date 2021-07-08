use super::binders::{ReadBinder, WriteBinder};
use super::config::ProtocolConfig;
use super::messages::Message;
use crate::crypto::signature::{PrivateKey, PublicKey, SignatureEngine};
use crate::network::connection_controller::{
    ConnectionClosureReason, ConnectionController, ConnectionControllerEvent, ConnectionId,
};
use futures::{future::try_join, stream::FuturesUnordered, StreamExt};
use rand::{rngs::StdRng, FromEntropy, RngCore};
use std::collections::{hash_map, HashMap, HashSet};
use std::error::Error;
use std::net::IpAddr;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{mpsc, oneshot};
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

#[derive(Clone, Copy, Debug)]
enum NodeCommand {
    Close,
}

#[derive(Clone, Debug)]
enum NodeEventType {
    ReceivedPeerList(Vec<IpAddr>),
    ReceivedBlock(String),       //put some date for the example
    ReceivedTransaction(String), //put some date for the example
    Closed(ConnectionClosureReason),
}

#[derive(Clone, Debug)]
struct NodeEvent(NodeId, NodeEventType);

pub struct ProtocolController {
    controller_event_rx: Receiver<ProtocolEvent>,
    controller_command_tx: Sender<ProtocolCommand>,
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
        let (controller_event_tx, controller_event_rx) = mpsc::channel::<ProtocolEvent>(1024);
        let (controller_command_tx, controller_command_rx) = mpsc::channel::<ProtocolCommand>(1024);
        let cfg_copy = cfg.clone();
        let protocol_controller_handle = tokio::spawn(async move {
            protocol_controller_fn(
                cfg_copy,
                self_node_id,
                private_key,
                connection_controller,
                controller_event_tx,
                controller_command_rx,
            )
            .await;
        });

        Ok(ProtocolController {
            controller_event_rx,
            controller_command_tx,
            protocol_controller_handle,
        })
    }

    //Receives the next ProtocolEvent from connected Node.
    //None is returned when all Sender halves have dropped, indicating that no further values can be sent on the channel
    pub async fn wait_event(&mut self) -> ProtocolEvent {
        self.controller_event_rx
            .recv()
            .await
            .expect("failed retrieving protocol controller event")
    }

    //stop the protocol controller
    pub async fn stop(self) {
        drop(self.controller_command_tx);
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
    let mut connection_controller_interface = connection_controller.get_upstream_interface();

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
                ConnectionControllerEvent::NewConnection((connection_id, socket)) => {
                    // add connection ID to running_handshakes
                    // launch async fn_handshake(connectionId, socket)
                    // add its handle to handshake_futures
                    if !running_handshakes.insert(connection_id) {
                        panic!("expect that the id is not already in running_handshakes");
                    }
                    let timeout_copy = Duration::from_secs_f32(cfg.message_timeout_seconds);
                    let handshake_fn_handle = tokio::spawn(async move {
                        fn_handshake(
                            connection_id,
                            socket,
                            self_node_id,
                            private_key,
                            timeout_copy,
                        )
                        .await
                    });
                    handshake_futures.push(handshake_fn_handle);
                }
                ConnectionControllerEvent::ConnectionBanned(connection_id) => {
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
                Some(Ok((new_connection_id, Ok((new_node_id, reader, writer))))) =>  {
                    // connection was banned in the meantime
                    if !running_handshakes.remove(&new_connection_id) {
                        connection_controller_interface
                            .connection_closed(new_connection_id, ConnectionClosureReason::Normal)
                            .await;
                        continue;
                    }

                    match active_nodes.entry(new_node_id) {
                        // we already have this node ID
                        hash_map::Entry::Occupied(_) => {
                            connection_controller_interface
                                .connection_closed(new_connection_id, ConnectionClosureReason::Normal)
                                .await;
                        },
                        // we don't have this node ID
                        hash_map::Entry::Vacant(entry) => {
                            // spawn fn_node_controller
                            let (node_command_tx, node_command_rx) = mpsc::channel::<NodeCommand>(1024);
                            let node_event_tx_clone = node_event_tx.clone();
                            let node_fn_handle = tokio::spawn(async move {
                                fn_node_controller(
                                    new_node_id,
                                    reader,
                                    writer,
                                    node_command_rx,
                                    node_event_tx_clone,
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
                    connection_controller_interface.connection_closed(connection_id, ConnectionClosureReason::Failed).await;
                },
                Some(Err(err)) => panic!("running handshake future await returned an error:{:?}", err),
                None => (),
            },

            // event received from a node
            Some(NodeEvent(from_node_id, event)) = node_event_rx.next() =>  match event {
                // received a list of peers
                NodeEventType::ReceivedPeerList(lst) => {
                    let (connection_id, _, _) = active_nodes.get(&from_node_id).expect("event from msising node");
                    connection_controller_interface.connection_alive(*connection_id).await;
                    connection_controller_interface
                        .merge_advertised_peer_list(lst)
                        .await;
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
                    connection_controller_interface
                        .connection_closed(connection_id, reason)
                        .await;
                    handle.await.expect("could not join node handle");
                }
            }

        } //end select!
    } //end loop

    {
        // close all active nodes
        let mut node_handle_set = FuturesUnordered::new();
        for (_, (connection_id, sender, handle)) in active_nodes.drain() {
            drop(sender);
            connection_controller_interface
                .connection_closed(connection_id, ConnectionClosureReason::Normal)
                .await;
            node_handle_set.push(handle);
        }
        while let Some(_) = node_handle_set.next().await {}
    }

    // wait for all running handshakes
    running_handshakes.clear();
    while let Some(res) = handshake_futures.next().await {
        match res {
            Ok((connection_id, Ok(_))) => {
                connection_controller_interface
                    .connection_closed(connection_id, ConnectionClosureReason::Normal)
                    .await;
            }
            Ok((connection_id, Err(_))) => {
                connection_controller_interface
                    .connection_closed(connection_id, ConnectionClosureReason::Failed)
                    .await;
            }
            Err(_) => panic!("failed gathering handshake futures"),
        }
    }

    // stop connection controller
    connection_controller.stop().await;
}

async fn fn_handshake(
    connection_id: ConnectionId,
    socket: TcpStream,
    our_node_id: NodeId, // NodeId.0 is our PublicKey
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
    let mut our_random_bytes = vec![0u8; 64];
    StdRng::from_entropy().fill_bytes(&mut our_random_bytes);

    // send handshake init future
    let send_init_msg = Message::HandshakeInitiation {
        public_key: our_node_id.0,
        random_bytes: our_random_bytes.clone(),
    };
    let send_init_fut = writer.send(&send_init_msg);

    // receive handshake init future
    let recv_init_fut = reader.next();

    // join send_init_fut and recv_init_fut with a timeout, and match result
    let (their_node_id, their_random_bytes) =
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
    if their_node_id == our_node_id {
        return (
            connection_id,
            Err("handshake announced own public key".into()),
        );
    }

    // sign their random bytes
    let signature_engine = SignatureEngine::new();
    let our_signature = signature_engine.sign(&their_random_bytes, &private_key);

    // send handshake reply future
    let send_reply_msg = Message::HandshakeReply {
        signature: our_signature,
    };
    let send_reply_fut = writer.send(&send_reply_msg);

    // receive handshake reply future
    let recv_reply_fut = reader.next();

    // join send_reply_fut and recv_reply_fut with a timeout, and match result
    let their_signature =
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
    if !signature_engine.verify(&our_random_bytes, &their_signature, &their_node_id.0) {
        return (
            connection_id,
            Err("invalid remote handshake signature".into()),
        );
    }

    (connection_id, Ok((their_node_id, reader, writer)))
}

async fn fn_node_controller(
    node_id: NodeId,
    reader: ReadBinder<OwnedReadHalf>,
    writer: WriteBinder<OwnedWriteHalf>,
    node_command_rx: Receiver<NodeCommand>,
    node_event_tx: Sender<NodeEvent>,
) {
    /*
        TODO
        - define writer_tx, writer_rx = mpsc(1024)
        - define writer_evt_tx, writer_evt_rx = mpsc(1024)
        - spawn fn_node_writer(writer, writer_evt_tx, writer_rx)
        - set a regular timer to ask for peers

    let secp = SignatureEngine::new();

        - Loop events:
            * reader.next()  // incoming data from reader
                TODO process

            * node_message_rx  // incoming command from controller
                TODO process

            * writer_evt_rx:  // incoming event from writer
                TODO process

            * peer ask timer
                TODO use writer_tx to ask for peer list

        - stop writer
        - notify controller of closure (with reason) through node_event_tx
    */
}

async fn fn_node_writer(/* writer, writer_evt_tx, writer_rx */) {
    // TODO
    // wait for writer_rx event
    //   - writer.send(data).await, with timeout
    //   - if error, notify through writer_evt_tx and return
}
