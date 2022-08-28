// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::{
    protocol_controller::ProtocolEventReceiver, ProtocolCommand, ProtocolCommandSender,
    ProtocolEvent,
};
use massa_models::{
    block::{BlockId, WrappedHeader},
    slot::Slot,
};
use massa_storage::Storage;
use massa_time::MassaTime;
use tokio::{sync::mpsc, time::sleep};

/// Mock of the protocol
/// TODO: Improve doc
pub struct MockProtocolController {
    protocol_command_rx: mpsc::Receiver<ProtocolCommand>,
    protocol_event_tx: mpsc::Sender<ProtocolEvent>,
}

impl MockProtocolController {
    /// Creates a new protocol mock
    /// TODO: Improve doc
    pub fn new() -> (Self, ProtocolCommandSender, ProtocolEventReceiver) {
        let (protocol_command_tx, protocol_command_rx) = mpsc::channel::<ProtocolCommand>(256);
        let (protocol_event_tx, protocol_event_rx) = mpsc::channel::<ProtocolEvent>(256);
        (
            MockProtocolController {
                protocol_event_tx,
                protocol_command_rx,
            },
            ProtocolCommandSender(protocol_command_tx),
            ProtocolEventReceiver(protocol_event_rx),
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

    /// Note: if you care about the operation set, use another method.
    pub async fn receive_block(&mut self, block_id: BlockId, slot: Slot, storage: Storage) {
        self.protocol_event_tx
            .send(ProtocolEvent::ReceivedBlock {
                block_id,
                slot,
                storage,
            })
            .await
            .expect("could not send protocol event");
    }

    /// Send a receive header to the protocol event channel
    pub async fn receive_header(&mut self, header: WrappedHeader) {
        let block_id = header.id;
        self.protocol_event_tx
            .send(ProtocolEvent::ReceivedBlockHeader { block_id, header })
            .await
            .expect("could not send protocol event");
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
