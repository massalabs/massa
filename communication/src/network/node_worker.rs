use super::{
    binders::{ReadBinder, WriteBinder},
    config::{NetworkConfig, CHANNEL_SIZE},
    messages::Message,
};
use crate::common::NodeId;
use crate::{error::CommunicationError, network::ConnectionClosureReason};
use crypto::hash::Hash;
use models::{Block, BlockHeader};
use std::collections::HashSet;
use std::net::IpAddr;
use tokio::{sync::mpsc, time::timeout};

#[derive(Clone, Debug)]
pub enum NodeCommand {
    /// Send given peer list to node.
    SendPeerList(Vec<IpAddr>),
    /// Send that block to node.
    SendBlock(Block),
    /// Send the header of a block to a node.
    SendBlockHeader(BlockHeader),
    /// Ask for a block from that node.
    AskForBlocks(HashSet<Hash>),
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
    ReceivedAskForBlocks(HashSet<Hash>),
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
    /// * cfg: Protocol configuration.
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
                trace!("before receiving command from writer_command_rx in node_worker run_loop");
                match writer_command_rx.recv().await {
                    Some(msg) => {
                        trace!("after receiving command from writer_command_rx in node_worker run_loop");
                        trace!("before sending msg from socket_writer in node_worker run_loop");
                        if let Err(_) =
                            timeout(write_timeout.to_duration(), socket_writer.send(&msg)).await
                        {
                            clean_exit = false;
                            break;
                        }
                        trace!("after sending msg from socket_writer in node_worker run_loop");
                    }
                    None => break,
                };
            }
            trace!("before sending clean_exit from writer_event_tx in node_worker run_loop");
            writer_event_tx
                .send(clean_exit)
                .await
                .expect("writer_evt_tx died"); //in a spawned task
            trace!("after sending clean_exit from writer_event_tx in node_worker run_loop");
        });

        let mut ask_peer_list_interval =
            tokio::time::interval(self.cfg.ask_peer_list_interval.to_duration());
        let mut exit_reason = ConnectionClosureReason::Normal;
        loop {
            trace!("before select! in node_worker run_loop");
            tokio::select! {
                // incoming socket data
                res = self.socket_reader.next() => match res {
                    Ok(Some((_, msg))) => {
                        trace!("after select res from self.socket_reader in node_worker run_loop");
                        match msg {
                            Message::Block(block) => {
                                trace!("before sending NodeEventType::ReceivedBlock from node_event_tx in node_worker run_loop");
                                self.node_event_tx.send(
                                    NodeEvent(self.node_id, NodeEventType::ReceivedBlock(block))
                                ).await.map_err(|_| CommunicationError::ChannelError("failed to send received block event".into()))?;
                                trace!("after sending NodeEventType::ReceivedBlock from node_event_tx in node_worker run_loop");
                            },
                            Message::BlockHeader(header) => {
                                trace!("before sending NodeEventType::ReceivedBlockHeader from node_event_tx in node_worker run_loop");
                                self.node_event_tx.send(
                                        NodeEvent(self.node_id, NodeEventType::ReceivedBlockHeader(header))
                                    ).await.map_err(|_| CommunicationError::ChannelError("failed to send received block header event".into()))?;
                                trace!("before sending NodeEventType::ReceivedBlockHeader from node_event_tx in node_worker run_loop");
                            },
                            Message::AskForBlocks(list) => {
                                trace!("before sending NodeEventType::ReceivedAskForBlock from node_event_tx in node_worker run_loop");
                                self.node_event_tx.send(
                                        NodeEvent(self.node_id, NodeEventType::ReceivedAskForBlocks(list.into_iter().collect()))
                                    ).await.map_err(|_| CommunicationError::ChannelError("failed to send received block header event".into()))?;
                                trace!("after sending NodeEventType::ReceivedAskForBlock from node_event_tx in node_worker run_loop");
                            }
                            Message::PeerList(pl) =>  {
                                trace!("before sending NodeEventType::ReceivedPeerList from node_event_tx in node_worker run_loop");
                                self.node_event_tx.send(
                                        NodeEvent(self.node_id, NodeEventType::ReceivedPeerList(pl))
                                    ).await.map_err(|_| CommunicationError::ChannelError("failed to send received peers list event".into()))?;
                                trace!("after sending NodeEventType::ReceivedPeerList from node_event_tx in node_worker run_loop");
                            }
                            Message::AskPeerList => {
                                trace!("before sending NodeEventType::AskedPeerList from node_event_tx in node_worker run_loop");
                                self.node_event_tx.send(
                                        NodeEvent(self.node_id, NodeEventType::AskedPeerList)
                                    ).await.map_err(|_| CommunicationError::ChannelError("failed to send asked block event".into()))?;
                                trace!("after sending NodeEventType::AskedPeerList from node_event_tx in node_worker run_loop");
                            }
                            Message::BlockNotFound(hash) => {
                                trace!("before sending NodeEventType::BlockNotFound from node_event_tx in node_worker run_loop");
                                self.node_event_tx.send(
                                    NodeEvent(self.node_id, NodeEventType::BlockNotFound(hash))
                                ).await.map_err(|_| CommunicationError::ChannelError("failed to send block not found event".into()))?;
                                trace!("after sending NodeEventType::BlockNotFound from node_event_tx in node_worker run_loop");
                            }
                            _ => {  // wrong message
                                exit_reason = ConnectionClosureReason::Failed;
                                break;
                            },
                        }
                    },
                    Ok(None)=> {
                        trace!("after receiving None from self.socket_reader in node_worker run_loop");
                        break
                    }, // peer closed cleanly
                    Err(_) => {  //stream error
                        trace!("after receiving _ from self.socket_reader in node_worker run_loop");
                        exit_reason = ConnectionClosureReason::Failed;
                        break;
                    },
                },

                // node command
                cmd = self.node_command_rx.recv() => {
                    trace!("after select receiving cmd from self.node_command_rx in node_worker run_loop");
                    match cmd {
                        Some(NodeCommand::Close(r)) => {
                            exit_reason = r;
                            break;
                        },
                        Some(NodeCommand::SendPeerList(ip_vec)) => {
                            trace!("before sending Message::PeerList from writer_command_tx in node_worker run_loop");
                            writer_command_tx.send(Message::PeerList(ip_vec)).await.map_err(
                                |_| CommunicationError::ChannelError("send peer list node command send failed".into())
                            )?;
                            trace!("after sending Message::PeerList from writer_command_tx in node_worker run_loop");
                        },
                        Some(NodeCommand::SendBlockHeader(header)) => {
                            trace!("before sending Message::BlockHeader from writer_command_tx in node_worker run_loop");
                            writer_command_tx.send(Message::BlockHeader(header)).await.map_err(
                                |_| CommunicationError::ChannelError("send peer block header node command send failed".into())
                            )?;
                            trace!("after sending Message::BlockHeader from writer_command_tx in node_worker run_loop");
                        },
                        Some(NodeCommand::SendBlock(block)) => {
                            trace!("before sending Message::Block from writer_command_tx in node_worker run_loop");
                            writer_command_tx.send(Message::Block(block)).await.map_err(
                                |_| CommunicationError::ChannelError("send peer block node command send failed".into())
                            )?;
                            trace!("after sending Message::Block from writer_command_tx in node_worker run_loop");
                        },
                        Some(NodeCommand::AskForBlocks(list)) => {
                            //cut hash list on sub list if exceed max_ask_blocks_per_message
                            trace!("before sending Message::AskForBlock from writer_command_tx in node_worker run_loop");
                            let list : Vec<_> = list.into_iter().collect();
                            for to_send_list in list.chunks(self.cfg.max_ask_blocks_per_message as usize) {
                                writer_command_tx.send(Message::AskForBlocks(to_send_list.iter().copied().collect())).await.map_err(
                                    |_| CommunicationError::ChannelError("ask peer block node command send failed".into())
                                )?;
                            }
                            trace!("after sending Message::AskForBlock from writer_command_tx in node_worker run_loop");
                        },
                        Some(NodeCommand::BlockNotFound(hash)) =>  {
                            trace!("before sending Message::BlockNotFound from writer_command_tx in node_worker run_loop");
                            writer_command_tx.send(Message::BlockNotFound(hash)).await.map_err(
                                |_| CommunicationError::ChannelError("send peer block not found node command send failed".into())
                            )?;
                            trace!("after sending Message::BlockNotFound from writer_command_tx in node_worker run_loop");
                        },
                        None => {
                            return Err(CommunicationError::UnexpectedProtocolControllerClosureError);
                        },
                    };
                }

                // writer event
                evt = writer_event_rx.recv() => {
                    trace!("after select receiving evt from writer_event_rx in node_worker run_loop");
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
                    trace!("after select receiving _ from ask_peer_list_interval in node_worker run_loop");
                    debug!("timer-based asking node_id={:?} for peer list", self.node_id);
                    massa_trace!("timer_ask_peer_list", {"node_id": self.node_id});
                    trace!("before sending Message::AskPeerList from writer_command_tx in node_worker run_loop");
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

        // notify protocol controller of closure
        trace!("before sending NodeEventType::Closed from node_event_tx in node_worker run_loop");
        self.node_event_tx
            .send(NodeEvent(self.node_id, NodeEventType::Closed(exit_reason)))
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("node closing event send failed".into())
            })?;
        trace!("after sending NodeEventType::Closed from node_event_tx in node_worker run_loop");
        Ok(())
    }
}
