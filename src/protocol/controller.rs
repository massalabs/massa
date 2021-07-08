use super::binders::{ReadBinder, WriteBinder};
use super::config::ProtocolConfig;
use super::messages::Message;
use crate::crypto::signature::{PrivateKey, PublicKey, SignatureEngine};
use crate::network::connection_controller::{ConnectionController, ConnectionId};
use futures::stream::FuturesUnordered;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};

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

async fn fn_handshake() { // todo parameters: connection ID, socket
                          /*
                              TODO
                              - socket.split_into
                                  let (mut socket_reader, mut socket_writer) = socket.split_into();
                                  let (reader, writer) = (ReadBinder::new(socket_reader), WriteBinder::new(socket_writer));
                              - wrap each side with the corresponding binder
                              - do the crypto handshake  (do not forget timeouts !)
                              - return (ConnectionId, Ok(NodeId, writer, reader)) or (ConnectionId, Err) if error
                          */
}

async fn fn_node_controller(/*  NodeId, reader, writer, node_message_rx, node_event_tx */) {
    /*
        TODO
        - define writer_tx, writer_rx = mpsc(1024)
        - define writer_evt_tx, writer_evt_rx = mpsc(1024)
        - spawn fn_node_writer(writer, writer_evt_tx, writer_rx)
        - set a regular timer to ask for peers


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
