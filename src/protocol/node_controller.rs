use super::binders::{ReadBinder, WriteBinder};
use super::config::ProtocolConfig;
use super::messages::Message;
use crate::crypto::signature::PublicKey;
use crate::logging::{debug, trace};
use crate::network::network_controller::ConnectionClosureReason;
use crate::structures::block::Block;
use futures::{future::FusedFuture, FutureExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub PublicKey);

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl std::fmt::Debug for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[derive(Clone, Debug)]
pub enum NodeCommand {
    SendPeerList(Vec<IpAddr>),
    SendBlock(Block),
    SendTransaction(String),
    Close,
}

#[derive(Clone, Debug)]
pub enum NodeEventType {
    AskedPeerList,
    ReceivedPeerList(Vec<IpAddr>),
    ReceivedBlock(Block),        //put some date for the example
    ReceivedTransaction(String), //put some date for the example
    Closed(ConnectionClosureReason),
}

#[derive(Clone, Debug)]
pub struct NodeEvent(pub NodeId, pub NodeEventType);

/// This function is launched in a new task
/// node_controller_fn is the link between
/// protocol_controller_fn (through node_command and node_event channels)
/// and node_writer_fn (through writer and writer_event channels)
///
/// Can panic if :
/// - node_event_tx died
/// - writer disappeared
/// - the protocol controller has not close everything before shuting down
/// - writer_evt_rx died
/// - writer_evt_tx already closed
/// - node_writer_handle already closed
/// - node_event_tx already closed
pub async fn node_controller_fn<ReaderT, WriterT: 'static>(
    cfg: ProtocolConfig,
    node_id: NodeId,
    mut socket_reader: ReadBinder<ReaderT>,
    socket_writer: WriteBinder<WriterT>,
    mut node_command_rx: Receiver<NodeCommand>,
    node_event_tx: Sender<NodeEvent>,
) where
    ReaderT: AsyncRead + Send + Sync + Unpin,
    WriterT: AsyncWrite + Send + Sync + Unpin,
{
    let (writer_command_tx, writer_command_rx) = mpsc::channel::<Message>(1024);
    let (writer_event_tx, writer_event_rx) = oneshot::channel::<bool>(); // true = OK, false = ERROR
    let mut fused_writer_event_rx = writer_event_rx.fuse();

    let cfg_copy = cfg.clone();
    let node_writer_handle = tokio::spawn(async move {
        node_writer_fn(cfg_copy, socket_writer, writer_event_tx, writer_command_rx).await;
    });

    let mut interval = tokio::time::interval(cfg.ask_peer_list_interval);

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
                debug!("timer-based asking node_id={:?} for peer list", node_id);
                massa_trace!("timer_ask_peer_list", {"node_id": node_id});
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

/// This function is spawned in a separated task
/// It communicates with node_controller_fn
/// through writer and writer event channels
/// Can panic if writer_event_tx died
/// Manages timeout while talking to anouther node
async fn node_writer_fn<WriterT: AsyncWrite + Send + Sync + Unpin>(
    cfg: ProtocolConfig,
    mut socket_writer: WriteBinder<WriterT>,
    writer_event_tx: oneshot::Sender<bool>,
    mut writer_command_rx: Receiver<Message>,
) {
    let write_timeout = cfg.message_timeout;
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
