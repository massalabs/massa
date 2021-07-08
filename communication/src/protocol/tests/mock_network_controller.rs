use crate::common::NodeId;
use crate::network::{NetworkCommand, NetworkCommandSender, NetworkEvent, NetworkEventReceiver};
use crypto::hash::Hash;
use models::{Block, BlockHeader};
use tokio::sync::mpsc;

const CHANNEL_SIZE: usize = 16;

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

    pub async fn wait_command(&mut self) -> Option<NetworkCommand> {
        Some(self.network_command_rx.recv().await?)
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

    pub async fn send_header(&mut self, source_node_id: NodeId, header: BlockHeader) {
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
            .send(NetworkEvent::ReceivedBlock(source_node_id, block))
            .await
            .expect("Couldn't send block to protocol.");
    }

    pub async fn send_ask_for_block(&mut self, source_node_id: NodeId, hash: Hash) {
        self.network_event_tx
            .send(NetworkEvent::AskedForBlock(source_node_id, hash))
            .await
            .expect("Couldn't send ask for block to protocol.");
    }
}
