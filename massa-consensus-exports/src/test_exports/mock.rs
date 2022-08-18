use massa_models::constants::CHANNEL_SIZE;
use massa_time::MassaTime;
use tokio::{sync::mpsc, time::sleep};

use crate::{
    commands::ConsensusCommand, events::ConsensusEvent, ConsensusCommandSender,
    ConsensusEventReceiver,
};

/// Mock for the consensus controller.
/// We will receive the commands in this mock and accept callback functions depending of the command in `wait_command`.
/// We will also send the events that can be received by listening to the `ConsensusEventReceiver`.
pub struct MockConsensusController {
    consensus_command_rx: mpsc::Receiver<ConsensusCommand>,
    consensus_event_tx: mpsc::Sender<ConsensusEvent>,
}

impl MockConsensusController {
    /// Create a new mock consensus controller.
    pub fn new() -> (Self, ConsensusCommandSender, ConsensusEventReceiver) {
        let (consensus_command_tx, consensus_command_rx) =
            mpsc::channel::<ConsensusCommand>(CHANNEL_SIZE);
        let (consensus_event_tx, consensus_event_rx) =
            mpsc::channel::<ConsensusEvent>(CHANNEL_SIZE);
        (
            MockConsensusController {
                consensus_command_rx,
                consensus_event_tx,
            },
            ConsensusCommandSender(consensus_command_tx),
            ConsensusEventReceiver(consensus_event_rx),
        )
    }

    /// wait command
    pub async fn wait_command<F, T>(&mut self, timeout: MassaTime, filter_map: F) -> Option<T>
    where
        F: Fn(ConsensusCommand) -> Option<T>,
    {
        let timer = sleep(timeout.into());
        tokio::pin!(timer);
        loop {
            tokio::select! {
                cmd_opt = self.consensus_command_rx.recv() => match cmd_opt {
                    Some(orig_cmd) => if let Some(res_cmd) = filter_map(orig_cmd) { return Some(res_cmd); },
                    None => panic!("Unexpected closure of network command channel."),
                },
                _ = &mut timer => return None
            }
        }
    }
}
