use super::{
    binders::{ReadBinder, WriteBinder},
    common::NodeId,
    config::{ProtocolConfig, CHANNEL_SIZE},
    messages::Message,
};
use crate::{error::CommunicationError, network::ConnectionClosureReason};
use crypto::hash::Hash;
use models::block::Block;
use std::net::IpAddr;
use tokio::{sync::mpsc, time::timeout};

/// Commands that node worker can manage.
#[derive(Clone, Debug)]
pub enum NodeCommand {
    /// Send given peer list to node.
    SendPeerList(Vec<IpAddr>),
    /// Send that block to node.
    SendBlock(Block),
    /// Send that transation to node.
    SendTransaction(String),
    /// Close the node worker.
    Close,
}

/// Event types that node worker can emit
#[derive(Clone, Debug)]
pub enum NodeEventType {
    AskedBlock(Hash),
    /// Node we are conneced to asked for advertized peers
    AskedPeerList,
    /// Node we are conneced to sent peer list
    ReceivedPeerList(Vec<IpAddr>),
    /// Node we are conneced to sent block
    ReceivedBlock(Block),
    /// Node we are conneced to sent transaction
    ReceivedTransaction(String),
    /// Connection with node was shut down for given reason
    Closed(ConnectionClosureReason),
}

/// Events node worker can emit.
/// Events are a tuple linking a node id to an event type
#[derive(Clone, Debug)]
pub struct NodeEvent(pub NodeId, pub NodeEventType);

/// Manages connections
/// One worker per node.
pub struct NodeWorker {
    /// Protocol configuration.
    cfg: ProtocolConfig,
    /// Node id associated to that worker.
    node_id: NodeId,
    /// Reader for incomming data.
    socket_reader: ReadBinder,
    /// Optional writer to send data.
    socket_writer_opt: Option<WriteBinder>,
    /// Channel to receive node commands.
    node_command_rx: mpsc::Receiver<NodeCommand>,
    /// Channel to send node events.
    node_event_tx: mpsc::Sender<NodeEvent>,
}

impl NodeWorker {
    /// Creates a new node worker
    ///
    /// # Arguments
    /// * cfg: Protocol configuration.
    /// * node_id: Node id associated to that worker.
    /// * socket_reader: Reader for incomming data.
    /// * socket_writer: Writer for sending data.
    /// * node_command_rx: Channel to receive node commands.
    /// * node_event_tx: Channel to send node events.
    pub fn new(
        cfg: ProtocolConfig,
        node_id: NodeId,
        socket_reader: ReadBinder,
        socket_writer: WriteBinder,
        node_command_rx: mpsc::Receiver<NodeCommand>,
        node_event_tx: mpsc::Sender<NodeEvent>,
    ) -> NodeWorker {
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
    pub async fn run_loop(mut self) -> Result<(), CommunicationError> {
        let (writer_command_tx, mut writer_command_rx) = mpsc::channel::<Message>(CHANNEL_SIZE);
        let (writer_event_tx, mut writer_event_rx) = mpsc::channel::<bool>(1);
        let mut socket_writer =
            self.socket_writer_opt
                .take()
                .ok_or(CommunicationError::GeneralProtocolError(
                    "NodeWorker call run_loop more than once".to_string(),
                ))?;
        let write_timeout = self.cfg.message_timeout;
        let node_writer_handle = tokio::spawn(async move {
            let mut clean_exit = true;
            loop {
                match writer_command_rx.recv().await {
                    Some(msg) => {
                        if let Err(_) =
                            timeout(write_timeout.to_duration(), socket_writer.send(&msg)).await
                        {
                            clean_exit = false;
                            break;
                        }
                    }
                    None => break,
                }
            }
            writer_event_tx
                .send(clean_exit)
                .await
                .expect("writer_evt_tx died"); //in a spawned task
        });

        let mut ask_peer_list_interval =
            tokio::time::interval(self.cfg.ask_peer_list_interval.to_duration());
        let mut exit_reason = ConnectionClosureReason::Normal;
        loop {
            tokio::select! {
                // incoming socket data
                res = self.socket_reader.next() => match res {
                    Ok(Some((_, msg))) => {
                        match msg {
                            Message::Block(block) => self.node_event_tx.send(
                                    NodeEvent(self.node_id, NodeEventType::ReceivedBlock(block))
                                ).await.map_err(|_| CommunicationError::ChannelError("failed to send received block event".into()))?,
                            Message::Transaction(tr) =>  self.node_event_tx.send(
                                    NodeEvent(self.node_id, NodeEventType::ReceivedTransaction(tr))
                                ).await.map_err(|_| CommunicationError::ChannelError("failed to send received transaction event".into()))?,
                            Message::PeerList(pl) =>  self.node_event_tx.send(
                                    NodeEvent(self.node_id, NodeEventType::ReceivedPeerList(pl))
                                ).await.map_err(|_| CommunicationError::ChannelError("failed to send received peers list event".into()))?,
                            Message::AskPeerList => self.node_event_tx.send(
                                    NodeEvent(self.node_id, NodeEventType::AskedPeerList)
                                ).await.map_err(|_| CommunicationError::ChannelError("failed to send asked block event".into()))?,
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
                cmd = self.node_command_rx.recv() => match cmd {
                    Some(NodeCommand::Close) => break,
                    Some(NodeCommand::SendPeerList(ip_vec)) => {
                        writer_command_tx.send(Message::PeerList(ip_vec)).await.map_err(
                            |_| CommunicationError::ChannelError("send peer list node command send failed".into())
                        )?;
                    }
                    Some(NodeCommand::SendBlock(block)) => {
                        writer_command_tx.send(Message::Block(block)).await.map_err(
                            |_| CommunicationError::ChannelError("send peer block node command send failed".into())
                        )?;
                    }
                    Some(NodeCommand::SendTransaction(transaction)) => {
                        writer_command_tx.send(Message::Transaction(transaction)).await.map_err(
                            |_| CommunicationError::ChannelError("send transaction node command send failed".into())
                        )?;
                    }
                    None => {
                        return Err(CommunicationError::UnexpectedProtocolControllerClosureError);
                    },
                },

                // writer event
                evt = writer_event_rx.recv() => match evt {
                    Some(s) => {
                        if !s {
                            exit_reason = ConnectionClosureReason::Failed;
                        }
                        break;
                    },
                    None => break
                },

                _ = ask_peer_list_interval.tick() => {
                    debug!("timer-based asking node_id={:?} for peer list", self.node_id);
                    massa_trace!("timer_ask_peer_list", {"node_id": self.node_id});
                    writer_command_tx.send(Message::AskPeerList).await.map_err(
                        |_| CommunicationError::ChannelError("writer send ask peer list failed".into())
                    )?;
                }
            }
        }

        // close writer
        drop(writer_command_tx);
        while let Some(_) = writer_event_rx.recv().await {}
        node_writer_handle.await?;

        // notify protocol controller of closure
        self.node_event_tx
            .send(NodeEvent(self.node_id, NodeEventType::Closed(exit_reason)))
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("node closing event send failed".into())
            })?;
        Ok(())
    }
}
