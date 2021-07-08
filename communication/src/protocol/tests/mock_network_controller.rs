use crate::common::NodeId;
use crate::network::{NetworkCommand, NetworkCommandSender, NetworkEvent, NetworkEventReceiver};
use crypto::hash::Hash;
use models::{Block, BlockHeader};
use time::UTime;
use tokio::{sync::mpsc, time::sleep};

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

    pub async fn wait_command<F>(&mut self, timeout: UTime, filter_map: F) -> Option<NetworkCommand>
    where
        F: Fn(NetworkCommand) -> Option<NetworkCommand>,
    {
        let timer = sleep(timeout.into());
        tokio::pin!(timer);
        loop {
            tokio::select! {
                cmd_opt = self.network_command_rx.recv() => match cmd_opt {
                    Some(orig_cmd) => if let Some(res_cmd) = filter_map(orig_cmd) { return Some(res_cmd); },
                    None => return None
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
            .send(NetworkEvent::ReceivedBlock {
                node: source_node_id,
                block,
            })
            .await
            .expect("Couldn't send block to protocol.");
    }

    pub async fn send_ask_for_block(&mut self, source_node_id: NodeId, hash: Hash) {
        self.network_event_tx
            .send(NetworkEvent::AskedForBlock {
                node: source_node_id,
                hash,
            })
            .await
            .expect("Couldn't send ask for block to protocol.");
    }
}
