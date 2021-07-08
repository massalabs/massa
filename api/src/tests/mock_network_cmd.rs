use communication::network::{NetworkCommand, NetworkCommandSender, PeerInfo};
use std::{collections::HashMap, net::IpAddr};
use tokio::{sync::mpsc, task::JoinHandle};

const CHANNEL_SIZE: usize = 16;

#[derive(Clone)]
pub struct NetworkMockData {
    pub peers: HashMap<IpAddr, PeerInfo>,
}

impl NetworkMockData {
    pub fn new() -> Self {
        NetworkMockData {
            peers: HashMap::new(),
        }
    }
}

pub fn start_mock_network(data: NetworkMockData) -> (JoinHandle<()>, NetworkCommandSender) {
    let (network_command_tx, mut network_command_rx) =
        mpsc::channel::<NetworkCommand>(CHANNEL_SIZE);

    let join_handle = tokio::spawn(async move {
        loop {
            match network_command_rx.recv().await {
                None => break,
                Some(NetworkCommand::GetPeers(sender)) => sender.send(data.peers.clone()).unwrap(),
                Some(_) => {}
            }
        }
    });

    (join_handle, NetworkCommandSender(network_command_tx))
}
