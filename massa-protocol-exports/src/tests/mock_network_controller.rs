// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crossbeam_channel::{after, bounded, select, Receiver, Sender};
use massa_models::{
    block_header::SecuredHeader, block_id::BlockId, endorsement::SecureShareEndorsement,
};
use massa_models::{
    config::CHANNEL_SIZE,
    node::NodeId,
    operation::{OperationId, SecureShareOperation},
};
use massa_network_exports::{AskForBlocksInfo, BlockInfoReply, NetworkCommand, NetworkEvent};
use massa_time::MassaTime;

/// mock network controller
pub struct MockNetworkController {
    network_command_rx: Receiver<NetworkCommand>,
    network_event_tx: Sender<NetworkEvent>,
}

impl MockNetworkController {
    /// new mock network controller
    pub fn new() -> (Self, Sender<NetworkCommand>, Receiver<NetworkEvent>) {
        let (network_command_tx, network_command_rx) = bounded::<NetworkCommand>(CHANNEL_SIZE);
        let (network_event_tx, network_event_rx) = bounded::<NetworkEvent>(CHANNEL_SIZE);
        (
            MockNetworkController {
                network_event_tx,
                network_command_rx,
            },
            network_command_tx,
            network_event_rx,
        )
    }

    /// wait command
    pub async fn wait_command<F, T>(&mut self, timeout: MassaTime, filter_map: F) -> Option<T>
    where
        F: Fn(NetworkCommand) -> Option<T>,
    {
        let timer = after(timeout.into());
        loop {
            select! {
                recv(self.network_command_rx) -> cmd_opt => match cmd_opt {
                    Ok(orig_cmd) => if let Some(res_cmd) = filter_map(orig_cmd) { return Some(res_cmd); },
                    Err(_) => panic!("Unexpected closure of network command channel."),
                },
                recv(timer) -> _ => return None
            }
        }
    }

    /// new connection
    pub async fn new_connection(&mut self, new_node_id: NodeId) {
        self.network_event_tx
            .send(NetworkEvent::NewConnection(new_node_id))
            .expect("Couldn't connect node to protocol.");
    }

    /// close connection
    pub async fn close_connection(&mut self, node_id: NodeId) {
        self.network_event_tx
            .send(NetworkEvent::ConnectionClosed(node_id))
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
            .expect("Couldn't send ask for block to protocol.");
    }
}
