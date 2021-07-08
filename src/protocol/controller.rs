use super::binders::{ReadBinder, WriteBinder};
use super::config::ProtocolConfig;
use super::messages::Message;
use crate::crypto::signature::{PrivateKey, PublicKey, SignatureEngine};
use crate::network::connection_controller::{ConnectionController, ConnectionId};
use futures::{future::try_join, stream::FuturesUnordered};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{timeout, Duration};

type BoxResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct NodeId(PublicKey);

enum NodeMessage {
    //TODO

// TODO Close  // asks the task to normally close the connection
}

pub struct ProtocolController {
    //TODO cummunication tx
//TODO event rx
//TODO handle
}

impl ProtocolController {
    pub async fn new(cfg: &ProtocolConfig) -> BoxResult<Self> {
        let connection_controller = ConnectionController::new(&cfg.network).await?;

        //TODO generate our own random PublicKey (and therefore NodeId) and keep private key

        let cfg_copy = cfg.clone();
        let handle = tokio::spawn(async move {
            protocol_controller_fn(cfg_copy, connection_controller).await;
        });

        Ok(ProtocolController {
            //TODO handle
        })
    }

    // TODO fn stop()
}

async fn protocol_controller_fn(cfg: ProtocolConfig, connection_controller: ConnectionController) {
    let running_handshakes: HashMap<ConnectionId, usize>; // number of running handshakes for each connection ID
                                                          //TODO let handshake_futures = FuturesUnordered::new();  //list of currently running handshake response futures (see fn_handshake)

    // list of active nodes we are connected to
    let active_nodes: HashMap<NodeId, (ConnectionId, (mpsc::Sender<NodeMessage> /*, JoinHandle */))> = HashMap::new();

    //TODO define node_event MPSC for events coming from nodes

    loop {
        //tokio::select! {
        //TODO incoming commands

        /*
            TODO connection_controller.wait_event()
                * new_connection (connectionId, socket)
                    - add connection ID to running_handshakes (set to 1 of doesn't exist, or increment if exists)
                    - launch async fn_handshake(connectionId, socket)
                    - add its handle to handshake_futures
                * connection_banned(connectionId)
                    remove the connectionId entry in running_handshakes
                    find all active_node with this ConnectionId and send a NodeMessage::Close
        */

        /*
            TODO handshake_futures
                - if succeeded (NodeId, ConnectionId, reader, writer)
                    - if ConnectionId in running_handshakes:
                        substract 1, remove if 0 reached
                      else : (banned)
                        notify connection_controller of normal closure
                        return;
                    - if NodeId in active_nodes:
                        (we already have this node)
                        notify connection_controller of normal closure
                        return;
                    - create a node_message_tx, node_message_rx MPSC
                    - spawn a fn_node_controller(NodeId, reader, writer, node_message_rx, node_event_tx)
                    - add NodeId : (node_message_tx, join_handle)) to active_nodes

                - if failed (ConnectionId)
                    - if in running_handshakes, substract 1, remove if 0 reached
                    - notify connection_controller of failure

        */

        /*
            TODO node_event_rx : event received from a node

        */
        //}
    }
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
    {
        use rand::{rngs::StdRng, FromEntropy, RngCore};
        StdRng::from_entropy().fill_bytes(&mut our_random_bytes);
    }

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

async fn fn_node_controller(/*  NodeId, reader, writer, node_message_rx, node_event_tx */) {
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
