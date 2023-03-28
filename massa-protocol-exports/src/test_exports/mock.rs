// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::{ProtocolCommand, ProtocolCommandSender};
use massa_models::block_id::BlockId;
use massa_time::MassaTime;
use tokio::{sync::mpsc, time::sleep};

/// Mock of the protocol
/// TODO: Improve doc
pub struct MockProtocolController {
    protocol_command_rx: mpsc::Receiver<ProtocolCommand>,
}

impl MockProtocolController {
    /// Creates a new protocol mock
    pub fn new() -> (Self, ProtocolCommandSender) {
        let (protocol_command_tx, protocol_command_rx) = mpsc::channel::<ProtocolCommand>(256);
        (
            MockProtocolController {
                protocol_command_rx,
            },
            ProtocolCommandSender(protocol_command_tx),
        )
    }

    /// Wait for a command sent to protocol and intercept it to pass it to `filter_map` function.
    pub async fn wait_command<F, T>(&mut self, timeout: MassaTime, filter_map: F) -> Option<T>
    where
        F: Fn(ProtocolCommand) -> Option<T>,
    {
        let timer = sleep(timeout.into());
        tokio::pin!(timer);
        loop {
            tokio::select! {
                cmd_opt = self.protocol_command_rx.recv() => match cmd_opt {
                    Some(orig_cmd) => if let Some(res_cmd) = filter_map(orig_cmd) { return Some(res_cmd); },
                    None => panic!("Unexpected closure of protocol command channel."),
                },
                _ = &mut timer => return None
            }
        }
    }

    /// Not implemented
    pub async fn receive_get_active_blocks(&mut self, _list: Vec<BlockId>) {}

    /// ignore all commands while waiting for a future
    pub async fn ignore_commands_while<FutureT: futures::Future + Unpin>(
        &mut self,
        mut future: FutureT,
    ) -> FutureT::Output {
        loop {
            tokio::select!(
                res = &mut future => return res,
                cmd = self.protocol_command_rx.recv() => match cmd {
                    Some(_) => {},
                    None => return future.await,  // if the protocol controlled dies, wait for the future to finish
                }
            );
        }
    }
}
