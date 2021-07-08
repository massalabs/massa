use super::super::{
    binders::{ReadBinder, WriteBinder},
    config::ProtocolConfig,
    messages::Message,
    protocol_controller::NodeId,
};
use crate::logging::debug;
use crate::network::network_controller::ConnectionClosureReason;
use crate::structures::block::Block;
use futures::{future::FusedFuture, FutureExt, StreamExt};
use std::net::IpAddr;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

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
    ReceivedBlock(Block),
    ReceivedTransaction(String),
    Closed(ConnectionClosureReason),
}

#[derive(Clone, Debug)]
pub struct NodeEvent(pub NodeId, pub NodeEventType);

pub struct NodeWorker<ReaderT: 'static, WriterT: 'static>
where
    ReaderT: AsyncRead + Send + Sync + Unpin,
    WriterT: AsyncWrite + Send + Sync + Unpin,
{
    cfg: ProtocolConfig,
    node_id: NodeId,
    socket_reader: ReadBinder<ReaderT>,
    socket_writer_opt: Option<WriteBinder<WriterT>>,
    node_command_rx: Receiver<NodeCommand>,
    node_event_tx: Sender<NodeEvent>,
}

impl<ReaderT: 'static, WriterT: 'static> NodeWorker<ReaderT, WriterT>
where
    ReaderT: AsyncRead + Send + Sync + Unpin,
    WriterT: AsyncWrite + Send + Sync + Unpin,
{
    pub fn new(
        cfg: ProtocolConfig,
        node_id: NodeId,
        socket_reader: ReadBinder<ReaderT>,
        socket_writer: WriteBinder<WriterT>,
        node_command_rx: Receiver<NodeCommand>,
        node_event_tx: Sender<NodeEvent>,
    ) -> NodeWorker<ReaderT, WriterT> {
        NodeWorker {
            cfg,
            node_id,
            socket_reader,
            socket_writer_opt: Some(socket_writer),
            node_command_rx,
            node_event_tx,
        }
    }

    /// node event loop. Consumes self.
    /// Can panic if :
    /// - node_event_tx died
    /// - writer disappeared
    /// - the protocol controller has not close everything before shuting down
    /// - writer_evt_rx died
    /// - writer_evt_tx already closed
    /// - node_writer_handle already closed
    /// - node_event_tx already closed
    pub async fn run_loop(mut self) {
        let (writer_command_tx, mut writer_command_rx) = mpsc::channel::<Message>(1024);
        let (writer_event_tx, writer_event_rx) = oneshot::channel::<bool>(); // true = OK, false = ERROR
        let mut fused_writer_event_rx = writer_event_rx.fuse();
        let mut socket_writer = self
            .socket_writer_opt
            .take()
            .expect("socket_writer disappeared");
        let write_timeout = self.cfg.message_timeout;
        let node_writer_handle = tokio::spawn(async move {
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
        });

        let mut ask_peer_list_interval = tokio::time::interval(self.cfg.ask_peer_list_interval);
        let mut exit_reason = ConnectionClosureReason::Normal;
        loop {
            tokio::select! {
                // incoming socket data
                res = self.socket_reader.next() => match res {
                    Ok(Some((_, msg))) => {
                        match msg {
                            Message::Block(block) => self.node_event_tx.send(
                                    NodeEvent(self.node_id, NodeEventType::ReceivedBlock(block))
                                ).await.expect("node_event_tx died"),
                            Message::Transaction(tr) =>  self.node_event_tx.send(
                                    NodeEvent(self.node_id, NodeEventType::ReceivedTransaction(tr))
                                ).await.expect("node_event_tx died"),
                            Message::PeerList(pl) =>  self.node_event_tx.send(
                                    NodeEvent(self.node_id, NodeEventType::ReceivedPeerList(pl))
                                ).await.expect("node_event_tx died"),
                            Message::AskPeerList => self.node_event_tx.send(
                                    NodeEvent(self.node_id, NodeEventType::AskedPeerList)
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
                cmd = self.node_command_rx.next() => match cmd {
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

                _ = ask_peer_list_interval.tick() => {
                    debug!("timer-based asking node_id={:?} for peer list", self.node_id);
                    massa_trace!("timer_ask_peer_list", {"node_id": self.node_id});
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
        self.node_event_tx
            .send(NodeEvent(self.node_id, NodeEventType::Closed(exit_reason)))
            .await
            .expect("node_event_tx already closed");
    }
}
