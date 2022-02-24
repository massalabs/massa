// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::constants::CHANNEL_SIZE;
use massa_pool::{PoolCommand, PoolCommandSender};
use massa_time::MassaTime;
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::sleep,
};

pub struct MockPoolController {
    pub pool_command_rx: mpsc::Receiver<PoolCommand>,
}

impl MockPoolController {
    pub fn new() -> (Self, PoolCommandSender) {
        let (pool_command_tx, pool_command_rx) = mpsc::channel::<PoolCommand>(CHANNEL_SIZE);
        (
            MockPoolController { pool_command_rx },
            PoolCommandSender(pool_command_tx),
        )
    }

    pub async fn wait_command<F, T>(&mut self, timeout: MassaTime, filter_map: F) -> Option<T>
    where
        F: Fn(PoolCommand) -> Option<T>,
    {
        let timer = sleep(timeout.into());
        tokio::pin!(timer);
        loop {
            tokio::select! {
                cmd_opt = self.pool_command_rx.recv() => match cmd_opt {
                    Some(orig_cmd) => if let Some(res_cmd) = filter_map(orig_cmd) { return Some(res_cmd); },
                    None => panic!("Unexpected closure of pool command channel."),
                },
                _ = &mut timer => return None
            }
        }
    }

    /*
    // ignore all commands while waiting for a future
    pub async fn ignore_commands_while<FutureT: futures::Future + Unpin>(
        &mut self,
        mut future: FutureT,
    ) -> FutureT::Output {
        loop {
            tokio::select!(
                res = &mut future => return res,
                cmd = self.pool_command_rx.recv() => match cmd {
                    Some(_) => {},
                    None => return future.await,  // if the controlled dies, wait for the future to finish
                }
            );
        }
    }
    */
}

// a structure that ignores pool commands
pub struct PoolCommandSink {
    stop_tx: oneshot::Sender<()>,
    handle: JoinHandle<MockPoolController>,
}

impl PoolCommandSink {
    pub async fn new(mut mock_pool_controller: MockPoolController) -> Self {
        let (stop_tx, stop_rx) = oneshot::channel();
        let handle = tokio::spawn(async move {
            tokio::pin!(stop_rx);
            loop {
                tokio::select! {
                    _ = &mut stop_rx => return mock_pool_controller,
                    cmd = mock_pool_controller.pool_command_rx.recv() => match cmd {
                        Some(PoolCommand::GetOperationBatch{response_tx, ..}) => {
                            response_tx.send(vec![]).unwrap();
                        },
                        Some(PoolCommand::GetEndorsements{response_tx, ..}) => {
                            response_tx.send(vec![]).unwrap();
                        }
                        Some(_) => {},
                        None => {
                            let _  = stop_rx.await;
                            return mock_pool_controller
                        }
                    }
                }
            }
        });
        PoolCommandSink { stop_tx, handle }
    }

    pub async fn stop(self) -> MockPoolController {
        drop(self.stop_tx);
        self.handle
            .await
            .expect("Failed to stop pool command sink.")
    }
}
