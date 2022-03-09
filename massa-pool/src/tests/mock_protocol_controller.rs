// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::prehash::Map;
use massa_models::{constants::CHANNEL_SIZE, EndorsementId, OperationId};
use massa_models::{SignedEndorsement, SignedOperation};
use massa_protocol_exports::{
    ProtocolCommand, ProtocolCommandSender, ProtocolPoolEvent, ProtocolPoolEventReceiver,
};
use massa_time::MassaTime;
use tokio::{sync::mpsc, time::sleep};

pub struct MockProtocolController {
    protocol_command_rx: mpsc::Receiver<ProtocolCommand>,
    pool_event_tx: mpsc::Sender<ProtocolPoolEvent>,
}

impl MockProtocolController {
    pub fn new() -> (Self, ProtocolCommandSender, ProtocolPoolEventReceiver) {
        let (protocol_command_tx, protocol_command_rx) =
            mpsc::channel::<ProtocolCommand>(CHANNEL_SIZE);
        let (pool_event_tx, pool_event_rx) = mpsc::channel::<ProtocolPoolEvent>(CHANNEL_SIZE);
        (
            MockProtocolController {
                protocol_command_rx,
                pool_event_tx,
            },
            ProtocolCommandSender(protocol_command_tx),
            ProtocolPoolEventReceiver(pool_event_rx),
        )
    }

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

    pub async fn received_operations(&mut self, operations: Map<OperationId, SignedOperation>) {
        self.pool_event_tx
            .send(ProtocolPoolEvent::ReceivedOperations {
                operations,
                propagate: true,
            })
            .await
            .expect("could not send protocol pool event");
    }

    pub async fn received_endorsements(
        &mut self,
        endorsements: Map<EndorsementId, SignedEndorsement>,
    ) {
        self.pool_event_tx
            .send(ProtocolPoolEvent::ReceivedEndorsements {
                endorsements,
                propagate: true,
            })
            .await
            .expect("could not send protocol pool event");
    }
}
