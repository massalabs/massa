use massa_models::node::NodeId;
use massa_network_exports::{ConnectionId, NetworkError, NetworkEvent, NodeCommand, NodeEvent};
use std::time::Duration;
use tokio::sync::mpsc::{self, error::SendTimeoutError};
use tracing::debug;

pub struct EventSender {
    /// Sender for network events
    controller_event_tx: mpsc::Sender<NetworkEvent>,
    /// Channel for sending node events.
    node_event_tx: mpsc::Sender<NodeEvent>,
    /// Max time spend to wait
    max_send_wait: Duration,
}

impl EventSender {
    pub fn new(
        controller_event_tx: mpsc::Sender<NetworkEvent>,
        node_event_tx: mpsc::Sender<NodeEvent>,
        max_send_wait: Duration,
    ) -> Self {
        Self {
            controller_event_tx,
            node_event_tx,
            max_send_wait,
        }
    }

    pub async fn send(&self, event: NetworkEvent) -> Result<(), NetworkError> {
        let result = self
            .controller_event_tx
            .send_timeout(event, self.max_send_wait)
            .await;
        match result {
            Ok(()) => return Ok(()),
            Err(SendTimeoutError::Closed(event)) => {
                debug!(
                    "Failed to send NetworkEvent due to channel closure: {:?}.",
                    event
                );
            }
            Err(SendTimeoutError::Timeout(event)) => {
                debug!("Failed to send NetworkEvent due to timeout: {:?}.", event);
            }
        }
        Err(NetworkError::ChannelError("Failed to send event.".into()))
    }

    /// Forward a message to a node worker. If it fails, notify upstream about connection closure.
    pub async fn forward(
        &self,
        node_id: NodeId,
        node: Option<&(ConnectionId, mpsc::Sender<NodeCommand>)>,
        message: NodeCommand,
    ) {
        if let Some((_, node_command_tx)) = node {
            if node_command_tx.send(message).await.is_err() {
                debug!(
                    "{}",
                    NetworkError::ChannelError("contact with node worker lost while trying to send it a message. Probably a peer disconnect.".into())
                );
            };
        } else {
            // We probably weren't able to send this event previously,
            // retry it now.
            let _ = self.send(NetworkEvent::ConnectionClosed(node_id)).await;
        }
    }

    pub fn clone_node_sender(&self) -> mpsc::Sender<NodeEvent> {
        self.node_event_tx.clone()
    }

    pub fn drop(self) {
        drop(self.node_event_tx)
    }
}

pub mod event_impl {
    use crate::network_worker::NetworkWorker;
    use massa_logging::massa_trace;
    use massa_models::signed::Signable;
    use massa_models::{
        node::NodeId,
        operation::{OperationIds, Operations},
        Block, BlockId, SignedEndorsement, SignedHeader,
    };
    use massa_network_exports::NodeCommand;
    use massa_network_exports::{NetworkError, NetworkEvent};
    use std::net::IpAddr;
    use tracing::{debug, info};
    macro_rules! evt_failed {
        ($err: ident) => {
            info!("Send network event failed {}", $err)
        };
    }

    // Implementation of the node event management functions
    pub fn on_received_peer_list(
        worker: &mut NetworkWorker,
        from: NodeId,
        list: &[IpAddr],
    ) -> Result<(), NetworkError> {
        debug!("node_id={} sent us a peer list ({} ips)", from, list.len());
        massa_trace!("peer_list_received", {
            "node_id": from,
            "ips": list
        });
        worker.peer_info_db.merge_candidate_peers(list)?;
        Ok(())
    }

    pub async fn on_received_block(
        worker: &mut NetworkWorker,
        from: NodeId,
        block: Block,
        serialized: Vec<u8>,
    ) -> Result<(), NetworkError> {
        massa_trace!(
            "network_worker.on_node_event receive NetworkEvent::ReceivedBlock",
            {"block_id": block.header.content.compute_id()?, "block": block, "node": from}
        );
        if let Err(err) = worker
            .event
            .send(NetworkEvent::ReceivedBlock {
                node: from,
                block,
                serialized,
            })
            .await
        {
            evt_failed!(err)
        }
        Ok(())
    }

    pub async fn on_received_ask_for_blocks(
        worker: &mut NetworkWorker,
        from: NodeId,
        list: Vec<BlockId>,
    ) {
        if let Err(err) = worker
            .event
            .send(NetworkEvent::AskedForBlocks { node: from, list })
            .await
        {
            evt_failed!(err)
        }
    }

    pub async fn on_received_block_header(
        worker: &mut NetworkWorker,
        from: NodeId,
        header: SignedHeader,
    ) -> Result<(), NetworkError> {
        massa_trace!(
            "network_worker.on_node_event receive NetworkEvent::ReceivedBlockHeader",
            {"hash": header.content.compute_hash()?, "header": header, "node": from}
        );
        if let Err(err) = worker
            .event
            .send(NetworkEvent::ReceivedBlockHeader {
                source_node_id: from,
                header,
            })
            .await
        {
            evt_failed!(err)
        }
        Ok(())
    }

    pub async fn on_asked_peer_list(
        worker: &mut NetworkWorker,
        from: NodeId,
    ) -> Result<(), NetworkError> {
        debug!("node_id={} asked us for peer list", from);
        massa_trace!("node_asked_peer_list", { "node_id": from });
        let peer_list = worker.peer_info_db.get_advertisable_peer_ips();
        if let Some((_, node_command_tx)) = worker.active_nodes.get(&from) {
            let res = node_command_tx
                .send(NodeCommand::SendPeerList(peer_list))
                .await;
            if res.is_err() {
                debug!(
                    "{}",
                    NetworkError::ChannelError("node command send send_peer_list failed".into(),)
                );
            }
        } else {
            massa_trace!("node asked us for peer list and disappeared", {
                "node_id": from
            })
        }
        Ok(())
    }

    pub async fn on_block_not_found(worker: &mut NetworkWorker, from: NodeId, block_id: BlockId) {
        massa_trace!(
            "network_worker.on_node_event receive NetworkEvent::BlockNotFound",
            { "id": block_id }
        );
        if let Err(err) = worker
            .event
            .send(NetworkEvent::BlockNotFound {
                node: from,
                block_id,
            })
            .await
        {
            evt_failed!(err)
        }
    }

    /// The node worker signal that he received some full `operations` from a
    /// node.
    ///
    /// Forward the event by sending a `[NetworkEvent::ReceivedOperations]`.
    /// See also `[massa_network_exports::NodeEventType::ReceivedOperations]`
    pub async fn on_received_operations(
        worker: &mut NetworkWorker,
        from: NodeId,
        operations: Operations,
    ) {
        massa_trace!(
            "network_worker.on_node_event receive NetworkEvent::ReceivedOperations",
            { "operations": operations }
        );
        if let Err(err) = worker
            .event
            .send(NetworkEvent::ReceivedOperations {
                node: from,
                operations,
            })
            .await
        {
            evt_failed!(err)
        }
    }

    /// The node worker signal that he received a batch of operation ids
    /// from another node.
    pub async fn on_received_operations_annoncement(
        worker: &mut NetworkWorker,
        from: NodeId,
        operation_ids: OperationIds,
    ) {
        massa_trace!(
            "network_worker.on_node_event receive NetworkEvent::ReceivedOperationAnnouncements",
            { "operations": operation_ids }
        );
        if let Err(err) = worker
            .event
            .send(NetworkEvent::ReceivedOperationAnnouncements {
                node: from,
                operation_ids,
            })
            .await
        {
            evt_failed!(err)
        }
    }

    /// The node worker signal that he received a list of operations required
    /// from another node.
    pub async fn on_received_ask_for_operations(
        worker: &mut NetworkWorker,
        from: NodeId,
        operation_ids: OperationIds,
    ) {
        massa_trace!(
            "network_worker.on_node_event receive NetworkEvent::ReceiveAskForOperations",
            { "operations": operation_ids }
        );
        if let Err(err) = worker
            .event
            .send(NetworkEvent::ReceiveAskForOperations {
                node: from,
                operation_ids,
            })
            .await
        {
            evt_failed!(err)
        }
    }

    pub async fn on_received_endorsements(
        worker: &mut NetworkWorker,
        from: NodeId,
        endorsements: Vec<SignedEndorsement>,
    ) {
        massa_trace!(
            "network_worker.on_node_event receive NetworkEvent::ReceivedEndorsements",
            { "endorsements": endorsements }
        );
        if let Err(err) = worker
            .event
            .send(NetworkEvent::ReceivedEndorsements {
                node: from,
                endorsements,
            })
            .await
        {
            evt_failed!(err)
        }
    }
}
