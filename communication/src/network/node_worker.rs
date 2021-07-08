use super::{
    binders::{ReadBinder, WriteBinder},
    config::{NetworkConfig, CHANNEL_SIZE},
    messages::Message,
};
use crate::common::NodeId;
use crate::{error::CommunicationError, network::ConnectionClosureReason};
use crypto::hash::Hash;
use models::SerializationContext;
use models::{Block, BlockHeader};
use std::net::IpAddr;
use tokio::{
    sync::mpsc,
    sync::mpsc::error::SendTimeoutError,
    time::{timeout, Duration},
};

#[derive(Clone, Debug)]
pub enum NodeCommand {
    /// Send given peer list to node.
    SendPeerList(Vec<IpAddr>),
    /// Send that block to node.
    SendBlock(Block),
    /// Send the header of a block to a node.
    SendBlockHeader(BlockHeader),
    /// Ask for a block from that node.
    AskForBlocks(Vec<Hash>),
    /// Close the node worker.
    Close(ConnectionClosureReason),
    /// Block not founf
    BlockNotFound(Hash),
}

/// Event types that node worker can emit
#[derive(Clone, Debug)]
pub enum NodeEventType {
    /// Node we are conneced to asked for advertized peers
    AskedPeerList,
    /// Node we are conneced to sent peer list
    ReceivedPeerList(Vec<IpAddr>),
    /// Node we are conneced to sent block
    ReceivedBlock(Block),
    /// Node we are conneced to sent block header
    ReceivedBlockHeader(BlockHeader),
    /// Node we are conneced to asks for a block.
    ReceivedAskForBlocks(Vec<Hash>),
    /// Connection with node was shut down for given reason
    Closed(ConnectionClosureReason),
    /// Didn't found given block,
    BlockNotFound(Hash),
}

/// Events node worker can emit.
/// Events are a tuple linking a node id to an event type
#[derive(Clone, Debug)]
pub struct NodeEvent(pub NodeId, pub NodeEventType);

/// Manages connections
/// One worker per node.
pub struct NodeWorker {
    /// Protocol configuration.
    cfg: NetworkConfig,
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
    /// * cfg: Network configuration.
    /// * serialization_context: SerializationContext instance
    /// * node_id: Node id associated to that worker.
    /// * socket_reader: Reader for incomming data.
    /// * socket_writer: Writer for sending data.
    /// * node_command_rx: Channel to receive node commands.
    /// * node_event_tx: Channel to send node events.
    pub fn new(
        cfg: NetworkConfig,
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

    async fn send_node_event(&self, event: NodeEvent) {
        let result = self
            .node_event_tx
            .send_timeout(
                event,
                Duration::from_millis(self.cfg.max_send_wait.to_millis()),
            )
            .await;
        match result {
            Ok(()) => {}
            Err(SendTimeoutError::Closed(event)) => {
                debug!(
                    "Failed to send NodeEvent due to channel closure: {:?}.",
                    event
                );
            }
            Err(SendTimeoutError::Timeout(event)) => {
                debug!("Failed to send NodeEvent due to timeout: {:?}.", event);
            }
        }
    }

    /// node event loop. Consumes self.
    pub async fn run_loop(
        mut self,
        serialization_context: SerializationContext,
    ) -> Result<(), CommunicationError> {
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
                };
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
                            Message::Block(block) => {
                                massa_trace!(
                                    "node_worker.run_loop. receive Message::Block",
                                    {"hash": block.header.content.compute_hash(&serialization_context)?, "block": block, "node": self.node_id}
                                );
                                self.send_node_event(NodeEvent(self.node_id, NodeEventType::ReceivedBlock(block))).await;
                            },
                            Message::BlockHeader(header) => {
                                massa_trace!(
                                    "node_worker.run_loop. receive Message::BlockHeader",
                                    {"hash": header.content.compute_hash(&serialization_context)?, "header": header, "node": self.node_id}
                                );
                                self.send_node_event(NodeEvent(self.node_id, NodeEventType::ReceivedBlockHeader(header))).await;
                            },
                            Message::AskForBlocks(list) => {
                                massa_trace!("node_worker.run_loop. receive Message::AskForBlocks", {"hashlist": list, "node": self.node_id});
                                self.send_node_event(NodeEvent(self.node_id, NodeEventType::ReceivedAskForBlocks(list))).await;
                            }
                            Message::PeerList(pl) =>  {
                                massa_trace!("node_worker.run_loop. receive Message::PeerList", {"peerlist": pl, "node": self.node_id});
                                self.send_node_event(NodeEvent(self.node_id, NodeEventType::ReceivedPeerList(pl))).await;
                            }
                            Message::AskPeerList => {
                                self.send_node_event(NodeEvent(self.node_id, NodeEventType::AskedPeerList)).await;
                            }
                            Message::BlockNotFound(hash) => {
                                massa_trace!("node_worker.run_loop. receive Message::BlockNotFound", {"hash": hash, "node": self.node_id});
                                self.send_node_event(NodeEvent(self.node_id, NodeEventType::BlockNotFound(hash))).await;
                            }
                            _ => {  // wrong message
                                exit_reason = ConnectionClosureReason::Failed;
                                break;
                            },
                        }
                    },
                    Ok(None)=> {
                        break
                    }, // peer closed cleanly
                    Err(_) => {  //stream error
                        exit_reason = ConnectionClosureReason::Failed;
                        break;
                    },
                },

                // node command
                cmd = self.node_command_rx.recv() => {
                    match cmd {
                        Some(NodeCommand::Close(r)) => {
                            exit_reason = r;
                            break;
                        },
                        Some(NodeCommand::SendPeerList(ip_vec)) => {
                            massa_trace!("node_worker.run_loop. send Message::PeerList", {"peerlist": ip_vec, "node": self.node_id});
                            writer_command_tx.send(Message::PeerList(ip_vec)).await.map_err(
                                |_| CommunicationError::ChannelError("send peer list node command send failed".into())
                            )?;
                        },
                        Some(NodeCommand::SendBlockHeader(header)) => {
                            massa_trace!("node_worker.run_loop. send Message::BlockHeader", {"hash": header.content.compute_hash(&serialization_context)?, "header": header, "node": self.node_id});
                            writer_command_tx.send(Message::BlockHeader(header)).await.map_err(
                                |_| CommunicationError::ChannelError("send peer block header node command send failed".into())
                            )?;
                        },
                        Some(NodeCommand::SendBlock(block)) => {
                            massa_trace!("node_worker.run_loop. send Message::Block", {"hash": block.header.content.compute_hash(&serialization_context)?, "block": block, "node": self.node_id});
                            writer_command_tx.send(Message::Block(block)).await.map_err(
                                |_| CommunicationError::ChannelError("send peer block node command send failed".into())
                            )?;
                            trace!("after sending Message::Block from writer_command_tx in node_worker run_loop");
                        },
                        Some(NodeCommand::AskForBlocks(list)) => {
                            //cut hash list on sub list if exceed max_ask_blocks_per_message
                            massa_trace!("node_worker.run_loop. send Message::AskForBlocks", {"hashlist": list, "node": self.node_id});
                            for to_send_list in list.chunks(self.cfg.max_ask_blocks_per_message as usize) {
                                writer_command_tx.send(Message::AskForBlocks(to_send_list.iter().copied().collect())).await.map_err(
                                    |_| CommunicationError::ChannelError("ask peer block node command send failed".into())
                                )?;
                            }
                        },
                        Some(NodeCommand::BlockNotFound(hash)) =>  {
                            massa_trace!("node_worker.run_loop. send Message::BlockNotFound", {"hash": hash, "node": self.node_id});
                            writer_command_tx.send(Message::BlockNotFound(hash)).await.map_err(
                                |_| CommunicationError::ChannelError("send peer block not found node command send failed".into())
                            )?;
                        },
                        None => {
                            return Err(CommunicationError::UnexpectedProtocolControllerClosureError);
                        },
                    };
                }

                // writer event
                evt = writer_event_rx.recv() => {
                    match evt {
                        Some(s) => {
                            if !s {
                                exit_reason = ConnectionClosureReason::Failed;
                            }
                            break;
                        },
                        None => break
                    };
                }

                _ = ask_peer_list_interval.tick() => {
                    debug!("timer-based asking node_id={:?} for peer list", self.node_id);
                    massa_trace!("node_worker.run_loop. timer_ask_peer_list", {"node_id": self.node_id});
                    massa_trace!("node_worker.run_loop.select.timer send Message::AskPeerList", {"node": self.node_id});
                    writer_command_tx.send(Message::AskPeerList).await.map_err(
                        |_| CommunicationError::ChannelError("writer send ask peer list failed".into())
                    )?;
                    trace!("after sending Message::AskPeerList from writer_command_tx in node_worker run_loop");
                }
            }
        }

        // close writer
        drop(writer_command_tx);
        while let Some(_) = writer_event_rx.recv().await {}
        node_writer_handle.await?;

        // Notify network controller of closure, while ignoring incoming commands to prevent deadlock.
        loop {
            tokio::select! {
                _ = self
                    .node_event_tx
                    .send(NodeEvent(self.node_id, NodeEventType::Closed(exit_reason))) => {
                    break;
                },
                _ = self.node_command_rx.recv() => {},
            }
        }
        Ok(())
    }
}
