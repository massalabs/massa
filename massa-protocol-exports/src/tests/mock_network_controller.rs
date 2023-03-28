// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{
    block_header::SecuredHeader, block_id::BlockId, endorsement::SecureShareEndorsement,
};
use massa_models::{
    config::CHANNEL_SIZE,
    node::NodeId,
    operation::{OperationId, SecureShareOperation},
};
use massa_network_exports::{
    AskForBlocksInfo, BlockInfoReply, NetworkCommand, NetworkCommandSender, NetworkEvent,
    NetworkEventReceiver,
};
use massa_time::MassaTime;
use tokio::{sync::mpsc, time::sleep};

/// mock network controller
pub struct MockNetworkController {
    network_command_rx: mpsc::Receiver<NetworkCommand>,
    network_event_tx: mpsc::Sender<NetworkEvent>,
}

impl MockNetworkController {
    /// new mock network controller
    pub fn new() -> (Self, NetworkCommandSender, NetworkEventReceiver) {
        let (network_command_tx, network_command_rx) =
            mpsc::channel::<NetworkCommand>(CHANNEL_SIZE);
        let (network_event_tx, network_event_rx) = mpsc::channel::<NetworkEvent>(CHANNEL_SIZE);
        (
            MockNetworkController {
                network_event_tx,
                network_command_rx,
            },
            NetworkCommandSender(network_command_tx),
            NetworkEventReceiver(network_event_rx),
        )
    }

    /// wait command
    pub async fn wait_command<F, T>(&mut self, timeout: MassaTime, filter_map: F) -> Option<T>
    where
        F: Fn(NetworkCommand) -> Option<T>,
    {
        let timer = sleep(timeout.into());
        tokio::pin!(timer);
        loop {
            tokio::select! {
                cmd_opt = self.network_command_rx.recv() => match cmd_opt {
                    Some(orig_cmd) => if let Some(res_cmd) = filter_map(orig_cmd) { return Some(res_cmd); },
                    None => panic!("Unexpected closure of network command channel."),
                },
                _ = &mut timer => return None
            }
        }
    }

    /// new connection
    pub async fn new_connection(&mut self, new_node_id: NodeId) {
        self.network_event_tx
            .send(NetworkEvent::NewConnection(new_node_id))
            .await
            .expect("Couldn't connect node to protocol.");
    }

    /// close connection
    pub async fn close_connection(&mut self, node_id: NodeId) {
        self.network_event_tx
            .send(NetworkEvent::ConnectionClosed(node_id))
            .await
            .expect("Couldn't connect node to protocol.");
    }

    /// send header
    /// todo inconsistency with names
    pub async fn send_header(&mut self, source_node_id: NodeId, header: SecuredHeader) {
        self.network_event_tx
            .send(NetworkEvent::ReceivedBlockHeader {
                source_node_id,
                header,
            })
            .await
            .expect("Couldn't send header to protocol.");
    }

    /// send operations
    /// todo inconsistency with names
    pub async fn send_operations(
        &mut self,
        source_node_id: NodeId,
        operations: Vec<SecureShareOperation>,
    ) {
        self.network_event_tx
            .send(NetworkEvent::ReceivedOperations {
                node: source_node_id,
                operations,
            })
            .await
            .expect("Couldn't send operations to protocol.");
    }

    /// send operation ids
    /// todo inconsistency with names
    pub async fn send_operation_batch(
        &mut self,
        source_node_id: NodeId,
        operation_ids: Vec<OperationId>,
    ) {
        self.network_event_tx
            .send(NetworkEvent::ReceivedOperationAnnouncements {
                node: source_node_id,
                operation_prefix_ids: operation_ids.iter().map(|id| id.into_prefix()).collect(),
            })
            .await
            .expect("Couldn't send operations to protocol.");
    }

    /// received ask for operation from node
    /// todo inconsistency with names
    pub async fn send_ask_for_operation(
        &mut self,
        source_node_id: NodeId,
        operation_ids: Vec<OperationId>,
    ) {
        self.network_event_tx
            .send(NetworkEvent::ReceiveAskForOperations {
                node: source_node_id,
                operation_prefix_ids: operation_ids.iter().map(|id| id.into_prefix()).collect(),
            })
            .await
            .expect("Couldn't send operations to protocol.");
    }

    /// send endorsements
    /// todo inconsistency with names
    pub async fn send_endorsements(
        &mut self,
        source_node_id: NodeId,
        endorsements: Vec<SecureShareEndorsement>,
    ) {
        self.network_event_tx
            .send(NetworkEvent::ReceivedEndorsements {
                node: source_node_id,
                endorsements,
            })
            .await
            .expect("Couldn't send endorsements to protocol.");
    }

    ///ask for block
    pub async fn send_ask_for_block(
        &mut self,
        source_node_id: NodeId,
        list: Vec<(BlockId, AskForBlocksInfo)>,
    ) {
        self.network_event_tx
            .send(NetworkEvent::AskedForBlocks {
                node: source_node_id,
                list,
            })
            .await
            .expect("Couldn't send ask for block to protocol.");
    }

    /// Send info about block
    pub async fn send_block_info(
        &mut self,
        source_node_id: NodeId,
        list: Vec<(BlockId, BlockInfoReply)>,
    ) {
        self.network_event_tx
            .send(NetworkEvent::ReceivedBlockInfo {
                node: source_node_id,
                info: list,
            })
            .await
            .expect("Couldn't send ask for block to protocol.");
    }
}
