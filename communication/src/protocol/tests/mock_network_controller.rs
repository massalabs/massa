use crate::network::{
    ConnectionId, NetworkCommand, NetworkCommandSender, NetworkEvent, NetworkEventReceiver,
    ReadHalf, WriteHalf,
};

use tokio::{
    io::{duplex, split},
    sync::mpsc,
};

const MAX_DUPLEX_BUFFER_SIZE: usize = 1024;
const CHANNEL_SIZE: usize = 16;

pub struct MockNetworkController {
    network_command_rx: mpsc::Receiver<NetworkCommand>,
    network_event_tx: mpsc::Sender<NetworkEvent>,
    cur_connection_id: ConnectionId,
}

pub fn get_duplex_pair() -> ((ReadHalf, WriteHalf), (ReadHalf, WriteHalf)) {
    let (duplex_1, duplex_2) = duplex(MAX_DUPLEX_BUFFER_SIZE);
    (split(duplex_1), split(duplex_2))
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
                cur_connection_id: ConnectionId::default(),
            },
            NetworkCommandSender(network_command_tx),
            NetworkEventReceiver(network_event_rx),
        )
    }

    pub async fn wait_command(&mut self) -> Option<NetworkCommand> {
        Some(self.network_command_rx.recv().await?)
    }

    // ignore all commands while waiting for a futrue
    pub async fn ignore_commands_while<FutureT: futures::Future + Unpin>(
        &mut self,
        mut future: FutureT,
    ) -> FutureT::Output {
        loop {
            tokio::select!(
                res = &mut future => return res,
                cmd = self.wait_command() => match cmd {
                    Some(_) => {},
                    None => return future.await,  // if the network controlled dies, wait for the future to finish
                }
            );
        }
    }
}
