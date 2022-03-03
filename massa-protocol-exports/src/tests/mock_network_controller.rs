// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::signed::Signed;
use massa_models::{constants::CHANNEL_SIZE, node::NodeId};
use massa_models::{Block, BlockHeader, BlockId, Endorsement, EndorsementId, Operation};
use massa_network::{NetworkCommand, NetworkCommandSender, NetworkEvent, NetworkEventReceiver};
use massa_time::MassaTime;
use tokio::{sync::mpsc, time::sleep};

pub struct MockNetworkController {
    network_command_rx: mpsc::Receiver<NetworkCommand>,
    network_event_tx: mpsc::Sender<NetworkEvent>,
}

impl MockNetworkController {
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

    pub async fn new_connection(&mut self, new_node_id: NodeId) {
        self.network_event_tx
            .send(NetworkEvent::NewConnection(new_node_id))
            .await
            .expect("Couldn't connect node to protocol.");
    }

    pub async fn close_connection(&mut self, node_id: NodeId) {
        self.network_event_tx
            .send(NetworkEvent::ConnectionClosed(node_id))
            .await
            .expect("Couldn't connect node to protocol.");
    }

    pub async fn send_header(
        &mut self,
        source_node_id: NodeId,
        header: Signed<BlockHeader, BlockId>,
    ) {
        self.network_event_tx
            .send(NetworkEvent::ReceivedBlockHeader {
                source_node_id,
                header,
            })
            .await
            .expect("Couldn't send header to protocol.");
    }

    pub async fn send_block(&mut self, source_node_id: NodeId, block: Block) {
        self.network_event_tx
            .send(NetworkEvent::ReceivedBlock {
                node: source_node_id,
                block,
            })
            .await
            .expect("Couldn't send block to protocol.");
    }

    pub async fn send_operations(&mut self, source_node_id: NodeId, operations: Vec<Operation>) {
        self.network_event_tx
            .send(NetworkEvent::ReceivedOperations {
                node: source_node_id,
                operations,
            })
            .await
            .expect("Couldn't send operations to protocol.");
    }

    pub async fn send_endorsements(
        &mut self,
        source_node_id: NodeId,
        endorsements: Vec<Signed<Endorsement, EndorsementId>>,
    ) {
        self.network_event_tx
            .send(NetworkEvent::ReceivedEndorsements {
                node: source_node_id,
                endorsements,
            })
            .await
            .expect("Couldn't send endorsements to protocol.");
    }

    pub async fn send_ask_for_block(&mut self, source_node_id: NodeId, list: Vec<BlockId>) {
        self.network_event_tx
            .send(NetworkEvent::AskedForBlocks {
                node: source_node_id,
                list,
            })
            .await
            .expect("Couldn't send ask for block to protocol.");
    }

    pub async fn send_block_not_found(&mut self, source_node_id: NodeId, block_id: BlockId) {
        self.network_event_tx
            .send(NetworkEvent::BlockNotFound {
                node: source_node_id,
                block_id,
            })
            .await
            .expect("Couldn't send ask for block to protocol.");
    }
}
