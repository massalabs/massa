// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::{ProtocolCommand, ProtocolCommandSender};
use crossbeam_channel::{after, bounded, select, Receiver};
use massa_models::block_id::BlockId;
use massa_time::MassaTime;

/// Mock of the protocol
/// TODO: Improve doc
pub struct MockProtocolController {
    protocol_command_rx: Receiver<ProtocolCommand>,
}

impl MockProtocolController {
    /// Creates a new protocol mock
    pub fn new() -> (Self, ProtocolCommandSender) {
        let (protocol_command_tx, protocol_command_rx) = bounded::<ProtocolCommand>(256);
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
        let timer = after(timeout.into());
        tokio::pin!(timer);
        loop {
            select! {
                recv(self.protocol_command_rx) -> cmd_opt => match cmd_opt {
                    Ok(orig_cmd) => if let Some(res_cmd) = filter_map(orig_cmd) { return Some(res_cmd); },
                    Err(_) => panic!("Unexpected closure of protocol command channel."),
                },
                recv(timer) -> _ => return None
            }
        }
    }

    /// Not implemented
    pub async fn receive_get_active_blocks(&mut self, _list: Vec<BlockId>) {}
}
