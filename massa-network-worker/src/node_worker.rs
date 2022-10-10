// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::{
    binders::{ReadBinder, WriteBinder},
    messages::Message,
};
use itertools::Itertools;
use massa_logging::massa_trace;
use massa_models::{
    config::{MAX_ENDORSEMENTS_PER_MESSAGE, NODE_SEND_CHANNEL_SIZE},
    node::NodeId,
    wrapped::Id,
};
use massa_network_exports::{ConnectionClosureReason, NetworkConfig, NetworkError, NetworkEvent, NodeCommand, NodeEvent, NodeEventType};
use tokio::{
    sync::mpsc,
    sync::mpsc::{
        error::{SendTimeoutError, TrySendError},
        Sender,
    },
    time::timeout,
};
use tracing::{debug, trace, warn};
use massa_time::MassaTime;

/// Manages connections
/// One worker per node.
pub struct NodeWorker {
    /// Protocol configuration.
    cfg: NetworkConfig,
    /// Node id associated to that worker.
    node_id: NodeId,
    /// Reader for incoming data.
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
    /// * `cfg`: Network configuration.
    /// * `node_id`: Node id associated to that worker.
    /// * `socket_reader`: Reader for incoming data.
    /// * `socket_writer`: Writer for sending data.
    /// * `node_command_rx`: Channel to receive node commands.
    /// * `node_event_tx`: Channel to send node events.
    /// * `storage`: Shared storage.
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
    pub async fn run_loop(mut self) -> Result<ConnectionClosureReason, NetworkError> {
        let (writer_command_tx, mut writer_command_rx) =
            mpsc::channel::<Message>(NODE_SEND_CHANNEL_SIZE);
        let mut socket_writer = self.socket_writer_opt.take().ok_or_else(|| {
            NetworkError::GeneralProtocolError(
                "NodeWorker call run_loop more than once".to_string(),
            )
        })?;
        let write_timeout = self.cfg.message_timeout;
        let node_id_copy = self.node_id;
        let node_writer_handle = tokio::spawn(async move {
            loop {
                match writer_command_rx.recv().await {
                    Some(to_send) => {
                        match timeout(write_timeout.to_duration(), socket_writer.send(&to_send))
                            .await
                        {
                            Err(_err) => {
                                massa_trace!("node_worker.run_loop.loop.writer_command_rx.recv.send.timeout", {
                                    "node": node_id_copy,
                                });
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::TimedOut,
                                    "node data writing timed out",
                                )
                                .into());
                            }
                            Ok(Err(err)) => {
                                massa_trace!("node_worker.run_loop.loop.writer_command_rx.recv.send.error", {
                                    "node": node_id_copy, "err":  format!("{}", err),
                                });
                                return Err(err);
                            }
                            Ok(Ok(id)) => {
                                massa_trace!("node_worker.run_loop.loop.writer_command_rx.recv.send.ok", {
                                    "node": node_id_copy, "msg_id": id,
                                })
                            }
                        }
                    }
                    None => {
                        massa_trace!("node_worker.run_loop.loop.writer_command_rx.recv. None", {});
                        break;
                    }
                };
            }
            Ok(())
        });
        tokio::pin!(node_writer_handle);
        let mut writer_joined = false;

        let node_reader_handle = tokio::spawn(async move {
            node_sender_handle(&mut self.socket_reader, &mut self.node_event_tx,
                               self.node_id, self.cfg.max_send_wait).await
        });
        tokio::pin!(node_reader_handle);
        let mut reader_joined = false;

        let mut ask_peer_list_interval =
            tokio::time::interval(self.cfg.ask_peer_list_interval.to_duration());
        let mut exit_reason = ConnectionClosureReason::Normal;
        let mut exit_reason_reader = ConnectionClosureReason::Normal;

        'select_loop: loop {
            /*
                select! without the "biased" modifier will randomly select the 1st branch to check,
                then will check the next ones in the order they are written.
                We choose this order:
                    * node_writer_handle (rare) to immediately register a stop and avoid wasting resources
                    * incoming socket data (high frequency): forward incoming data in priority to avoid contention
                    * node commands (high frequency): try to send, fail on contention
                    * ask peers: low frequency, non-critical
            */
            tokio::select! {
                res = &mut node_writer_handle => {
                    writer_joined = true;
                    match res {
                        Err(err) => {
                            massa_trace!("node_worker.run_loop.node_writer_handle.panic", {"node": self.node_id, "err": format!("{}", err)});
                            warn!("writer exited unexpectedly for node {}", self.node_id);
                            if exit_reason != ConnectionClosureReason::Banned {
                                exit_reason = ConnectionClosureReason::Failed;
                            }
                            break;
                        },
                        Ok(Err(err)) => {
                            massa_trace!("node_worker.run_loop.node_writer_handle.error", {"node": self.node_id, "err": format!("{}", err)});
                            if exit_reason != ConnectionClosureReason::Banned {
                                exit_reason = ConnectionClosureReason::Failed;
                            }
                            break;
                        },
                        Ok(Ok(())) => {
                            massa_trace!("node_worker.run_loop.node_writer_handle.clean_exit", {"node": self.node_id});
                            break;
                        }
                    }
                },

                // incoming socket data
                res = &mut node_reader_handle => {
                    reader_joined = true;
                    exit_reason_reader = match res {
                        Ok(r) => {
                            ConnectionClosureReason::Normal
                        },
                        Err(_) => {
                            ConnectionClosureReason::Failed
                        }
                    };
                    break;
                }
                // node command
                cmd = self.node_command_rx.recv() => {
                    match cmd {
                        Some(NodeCommand::Close(r)) => {
                            exit_reason = r;
                            break;
                        },
                        Some(NodeCommand::SendPeerList(ip_vec)) => {
                            massa_trace!("node_worker.run_loop. send Message::PeerList", {"peerlist": ip_vec, "node": self.node_id});
                            if try_send_to_node(&writer_command_tx, Message::PeerList(ip_vec), self.node_id).is_err() {
                                break;
                            }
                        },
                        Some(NodeCommand::SendBlockHeader(header)) => {
                            massa_trace!("node_worker.run_loop. send Message::BlockHeader", {"hash": header.id, "node": self.node_id});
                            if try_send_to_node(&writer_command_tx, Message::BlockHeader(header), self.node_id).is_err() {
                                break;
                            }
                        },
                        Some(NodeCommand::AskForBlocks(list)) => {
                            // cut hash list on sub list if exceed max_ask_blocks_per_message
                            massa_trace!("node_worker.run_loop. send Message::AskForBlocks", {"hashlist": list, "node": self.node_id});
                            for to_send_list in list.chunks(self.cfg.max_ask_blocks as usize) {
                                if try_send_to_node(&writer_command_tx, Message::AskForBlocks(to_send_list.to_vec()), self.node_id).is_err() {
                                    break 'select_loop;
                                }
                            }
                        },
                        Some(NodeCommand::ReplyForBlocks(list)) => {
                            // cut hash list on sub list if exceed max_ask_blocks_per_message
                            massa_trace!("node_worker.run_loop. send Message::ReplyForBlocks", {"hashlist": list, "node": self.node_id});
                            for to_send_list in list.chunks(self.cfg.max_ask_blocks as usize) {
                                if try_send_to_node(&writer_command_tx, Message::ReplyForBlocks(to_send_list.to_vec()), self.node_id).is_err() {
                                    break 'select_loop;
                                }
                            }
                        },
                        Some(NodeCommand::SendOperations(operations)) => {
                            massa_trace!("node_worker.run_loop. send Message::SendOperations", {"node": self.node_id, "operations": operations});
                            for chunk in operations.chunks(self.cfg.max_operations_per_message as usize) {
                                if try_send_to_node(&writer_command_tx, Message::Operations(chunk.to_vec()), self.node_id).is_err() {
                                    break 'select_loop;
                                }
                            }
                        },
                        Some(NodeCommand::SendOperationAnnouncements(operation_prefix_ids)) => {
                            massa_trace!("node_worker.run_loop. send Message::OperationsAnnouncement", {"node": self.node_id, "operation_ids": operation_prefix_ids});
                            for chunk in operation_prefix_ids
                            .into_iter()
                            .chunks(self.cfg.max_operations_per_message as usize)
                            .into_iter()
                            .map(|chunk| chunk.collect()) {
                                if try_send_to_node(&writer_command_tx, Message::OperationsAnnouncement(chunk), self.node_id).is_err() {
                                    break 'select_loop;
                                }
                            }
                        }
                        Some(NodeCommand::AskForOperations(operation_prefix_ids)) => {
                            //massa_trace!("node_worker.run_loop. send Message::AskForOperations", {"node": self.node_id, "operation_ids": operation_ids});
                            massa_trace!(
                                "node_worker.run_loop. send Message::AskForOperations",
                                {"node": self.node_id, "operation_ids": operation_prefix_ids}
                            );
                            for chunk in operation_prefix_ids
                            .into_iter()
                            .chunks(self.cfg.max_operations_per_message as usize)
                            .into_iter()
                            .map(|chunk| chunk.collect()) {
                                if try_send_to_node(&writer_command_tx, Message::AskForOperations(chunk), self.node_id).is_err() {
                                    break 'select_loop;
                                }
                            }
                        }
                        Some(NodeCommand::SendEndorsements(endorsements)) => {
                            massa_trace!("node_worker.run_loop. send Message::SendEndorsements", {"node": self.node_id, "endorsements": endorsements});
                            // cut endorsement list if it exceed max_endorsements_per_message
                            for to_send_list in endorsements.chunks(MAX_ENDORSEMENTS_PER_MESSAGE as usize) {
                                if try_send_to_node(&writer_command_tx, Message::Endorsements(to_send_list.to_vec()), self.node_id).is_err() {
                                    break 'select_loop;
                                }
                            }
                        },
                        None => {
                            // Note: this should never happen,
                            // since it implies the network worker dropped its node command sender
                            // before having shut-down the node and joined on its handle.
                            return Err(NetworkError::UnexpectedNodeCommandChannelClosure);
                        },
                    };
                },

                _ = ask_peer_list_interval.tick() => {
                    debug!("timer-based asking node_id={} for peer list", self.node_id);
                    massa_trace!("node_worker.run_loop. timer_ask_peer_list", {"node_id": self.node_id});
                    massa_trace!("node_worker.run_loop.select.timer send Message::AskPeerList", {"node": self.node_id});
                    match writer_command_tx.try_send(Message::AskPeerList) {
                        Ok(()) => {},
                        Err(TrySendError::Full(_)) => {
                            warn!("node channel full: {}", self.node_id);
                        },
                        Err(TrySendError::Closed(_)) => {
                            return Err(NetworkError::ChannelError("writer send ask peer list failed".into()));
                        }
                    };
                    trace!("after sending Message::AskPeerList from writer_command_tx in node_worker run_loop");
                }
            }
        }

        // Note: since we close the channel here,
        // if the network worker tries to send additional commands,
        // those sends will fail with an error.
        self.node_command_rx.close();

        // 1. Close writer command channel.
        drop(writer_command_tx);

        // 2. Join on the writer handle.
        if !writer_joined {
            match node_writer_handle.await {
                Err(err) => {
                    massa_trace!("node_worker.run_loop.cleanup.node_writer_handle.panic", {"node": self.node_id, "err": format!("{}", err)});
                    warn!("writer exited unexpectedly for node {}", self.node_id);
                    exit_reason = ConnectionClosureReason::Failed;
                }
                Ok(Err(err)) => {
                    massa_trace!("node_worker.run_loop.cleanup.node_writer_handle.error", {"node": self.node_id, "err": format!("{}", err)});
                    exit_reason = ConnectionClosureReason::Failed;
                }
                Ok(Ok(())) => {
                    massa_trace!("node_worker.run_loop.cleanup.node_writer_handle.clean_exit", {"node": self.node_id});
                }
            }
        }

        Ok(exit_reason)
    }
}

async fn node_sender_handle(socker_reader: &mut ReadBinder, node_event_tx: &mut Sender<NodeEvent>,
                            node_id: NodeId, max_send_wait: MassaTime) -> ConnectionClosureReason {

    let mut exit_reason = ConnectionClosureReason::Normal;

    loop {
        match socker_reader.next().await {
            Ok(Some((index, msg))) => {

                massa_trace!(
                            "node_worker.run_loop. receive self.socket_reader.next()", {"index": index});
                match msg {
                    Message::BlockHeader(header) => {
                        massa_trace!(
                                    "node_worker.run_loop. receive Message::BlockHeader",
                                    {"block_id": header.id.get_hash(), "header": header, "node": node_id}
                                );
                        let event = NodeEvent(node_id, NodeEventType::ReceivedBlockHeader(header));
                        send_node_event(node_event_tx, event, max_send_wait).await
                    },
                    Message::AskForBlocks(list) => {
                        massa_trace!("node_worker.run_loop. receive Message::AskForBlocks", {"hashlist": list, "node": node_id});
                        let event = NodeEvent(node_id, NodeEventType::ReceivedAskForBlocks(list));
                        send_node_event(node_event_tx, event, max_send_wait).await
                    }
                    Message::ReplyForBlocks(list) => {
                        massa_trace!("node_worker.run_loop. receive Message::AskForBlocks", {"hashlist": list, "node": node_id});
                        let event = NodeEvent(node_id, NodeEventType::ReceivedReplyForBlocks(list));
                        send_node_event(node_event_tx, event, max_send_wait).await
                    }
                    Message::PeerList(pl) => {
                        massa_trace!("node_worker.run_loop. receive Message::PeerList", {"peerlist": pl, "node": node_id});
                        let event = NodeEvent(node_id, NodeEventType::ReceivedPeerList(pl));
                        send_node_event(node_event_tx, event, max_send_wait).await
                    }
                    Message::AskPeerList => {
                        let event = NodeEvent(node_id, NodeEventType::AskedPeerList);
                        send_node_event(node_event_tx, event, max_send_wait).await
                    }
                    Message::Operations(operations) => {
                        massa_trace!(
                                    "node_worker.run_loop. receive Message::Operations: ",
                                    {"node": node_id, "operations": operations}
                                );
                        //massa_trace!("node_worker.run_loop. receive Message::Operations", {"node": self.node_id, "operations": operations});
                        let event = NodeEvent(node_id, NodeEventType::ReceivedOperations(operations));
                        send_node_event(node_event_tx, event, max_send_wait).await
                    }
                    Message::AskForOperations(operation_prefix_ids) => {
                        massa_trace!(
                                    "node_worker.run_loop. receive Message::AskForOperations: ",
                                    {"node": node_id, "operation_ids": operation_prefix_ids}
                                );
                        //massa_trace!("node_worker.run_loop. receive Message::AskForOperations", {"node": self.node_id, "operations": operation_ids});
                        let event = NodeEvent(node_id, NodeEventType::ReceivedAskForOperations(operation_prefix_ids));
                        send_node_event(node_event_tx, event, max_send_wait).await
                    }
                    Message::OperationsAnnouncement(operation_prefix_ids) => {
                        massa_trace!("node_worker.run_loop. receive Message::OperationsBatch", {"node": node_id, "operation_prefix_ids": operation_prefix_ids});
                        let event = NodeEvent(node_id, NodeEventType::ReceivedOperationAnnouncements(operation_prefix_ids));
                        send_node_event(node_event_tx, event, max_send_wait).await
                    }
                    Message::Endorsements(endorsements) => {
                        massa_trace!("node_worker.run_loop. receive Message::Endorsement", {"node": node_id, "endorsements": endorsements});
                        let event =NodeEvent(node_id, NodeEventType::ReceivedEndorsements(endorsements));
                        send_node_event(node_event_tx, event, max_send_wait).await
                    }
                    _ => {
                        // TODO: Write a more user-friendly warning/logout after several consecutive fails? see #1082
                        massa_trace!("node_worker.run_loop.self.socket_reader.next(). Unexpected message Warning", {});
                    },
                }
            },
            Ok(None)=> {
                massa_trace!("node_worker.run_loop.self.socket_reader.next(). Ok(None) Error", {});
                break
            }, // peer closed cleanly
            Err(err) => {  // stream error
                massa_trace!("node_worker.run_loop.self.socket_reader.next(). receive error", {"error": format!("{}", err)});
                exit_reason = ConnectionClosureReason::Failed;
                break;
            },
        }
    }

    return exit_reason;
}

async fn send_node_event(node_event_tx: &mut Sender<NodeEvent>, event: NodeEvent, max_send_wait: MassaTime) {

    let result = node_event_tx
        // .send_timeout(event, max_send_wait.to_duration())
        .send_timeout(event, tokio::time::Duration::from_millis(10000))
        .await;
    match result {
        Ok(()) => {}
        Err(SendTimeoutError::Closed(event)) => {
            warn!(
                    "Failed to send NodeEvent due to channel closure: {:?}.",
                    event
                );
        }
        Err(SendTimeoutError::Timeout(event)) => {
            warn!("Failed to send NodeEvent due to timeout: {:?}.", event);
        }
    }
}

pub fn try_send_to_node(
    sender: &Sender<Message>,
    msg: Message,
    node_id: NodeId,
) -> Result<(), NetworkError> {
    match sender.try_send(msg) {
        Err(TrySendError::Full(_)) => {
            warn!(
                    "failed sending message to node {}: send channel full",
                    node_id
                );
            Ok(())
        }
        Err(TrySendError::Closed(_)) => {
            warn!("failed sending message disconnected {}.", node_id);
            Err(NetworkError::ChannelError(
                "failed sending message to node: channel closed".into(),
            ))
        }
        Ok(_) => Ok(()),
    }
}
