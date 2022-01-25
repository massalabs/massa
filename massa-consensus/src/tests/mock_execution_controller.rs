// Copyright (c) 2021 MASSA LABS <info@massa.net>

use massa_execution::{
    ExecutionCommand, ExecutionCommandSender, ExecutionEvent, ExecutionEventReceiver,
};
use massa_models::prehash::Map;
use massa_models::{Block, BlockId};
use massa_time::MassaTime;
use tokio::{
    sync::mpsc::{channel, unbounded_channel, Receiver, Sender, UnboundedSender},
    time::sleep,
};

const CHANNEL_SIZE: usize = 256;

#[allow(dead_code)]
pub struct MockExecutionController {
    execution_command_sender: Sender<ExecutionCommand>,
    execution_command_receiver: Receiver<ExecutionCommand>,
    event_sender: UnboundedSender<ExecutionEvent>,
}

impl MockExecutionController {
    pub fn new() -> (Self, ExecutionCommandSender, ExecutionEventReceiver) {
        let (event_sender, event_rx) = unbounded_channel::<ExecutionEvent>();
        let (execution_command_sender, execution_command_receiver) =
            channel::<ExecutionCommand>(CHANNEL_SIZE);
        (
            MockExecutionController {
                execution_command_sender: execution_command_sender.clone(),
                execution_command_receiver,
                event_sender,
            },
            ExecutionCommandSender(execution_command_sender),
            ExecutionEventReceiver(event_rx),
        )
    }

    #[allow(dead_code)]
    pub async fn wait_command<F, T>(&mut self, timeout: MassaTime, filter_map: F) -> Option<T>
    where
        F: Fn(ExecutionCommand) -> Option<T>,
    {
        let timer = sleep(timeout.into());
        tokio::pin!(timer);
        loop {
            tokio::select! {
                cmd_opt = self.execution_command_receiver.recv() => match cmd_opt {
                    Some(orig_cmd) => if let Some(res_cmd) = filter_map(orig_cmd) { return Some(res_cmd); },
                    None => panic!("Unexpected closure of execution command command channel."),
                },
                _ = &mut timer => return None
            }
        }
    }

    #[allow(dead_code)]
    pub async fn blockclique_changed(
        &mut self,
        blockclique: Map<BlockId, Block>,
        finalized_blocks: Map<BlockId, Block>,
    ) {
        self.execution_command_sender
            .send(ExecutionCommand::BlockCliqueChanged {
                blockclique,
                finalized_blocks,
            })
            .await
            .expect("could not send execution event");
    }

    #[allow(dead_code)]
    pub async fn ignore_commands_while<FutureT: futures::Future + Unpin>(
        &mut self,
        mut future: FutureT,
    ) -> FutureT::Output {
        loop {
            tokio::select!(
                res = &mut future => return res,
                cmd = self.execution_command_receiver.recv() => match cmd {
                    Some(_) => {},
                    None => return future.await
                }
            );
        }
    }
}
