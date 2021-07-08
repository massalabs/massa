use communication::protocol::{ProtocolCommand, ProtocolCommandSender};
use tokio::{sync::mpsc, task::JoinHandle};

const CHANNEL_SIZE: usize = 16;

pub fn start_mock_protocol() -> (JoinHandle<()>, ProtocolCommandSender) {
    let (protocol_command_tx, mut protocol_command_rx) =
        mpsc::channel::<ProtocolCommand>(CHANNEL_SIZE);

    let join_handle = tokio::spawn(async move {
        loop {
            match protocol_command_rx.recv().await {
                None => break,
                Some(_) => {}
            }
        }
    });

    (join_handle, ProtocolCommandSender(protocol_command_tx))
}
