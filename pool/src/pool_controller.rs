use super::{
    config::{PoolConfig, CHANNEL_SIZE},
    error::PoolError,
    pool_worker::{PoolCommand, PoolManagementCommand, PoolWorker},
};
use logging::{debug, massa_trace};
use models::SerializationContext;
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::{sleep_until, Sleep},
};

/// Creates a new pool controller.
///
/// # Arguments
/// * cfg: pool configuration
/// * protocol_command_sender: a ProtocolCommandSender instance to send commands to Protocol.
/// * protocol_pool_event_receiver: a ProtocolPoolEventReceiver instance to receive pool events from Protocol.
pub async fn start_pool_controller(
    _serialization_context: SerializationContext,
) -> Result<(PoolCommandSender, PoolManager), PoolError> {
    debug!("starting pool controller");
    massa_trace!("pool.pool_controller.start_pool_controller", {});

    // start worker
    let (command_tx, command_rx) = mpsc::channel::<PoolCommand>(CHANNEL_SIZE);
    let (manager_tx, manager_rx) = mpsc::channel::<PoolManagementCommand>(1);
    let join_handle = tokio::spawn(async move {
        let res = PoolWorker::new(command_rx, manager_rx)?.run_loop().await;
        match res {
            Err(err) => {
                error!("pool worker crashed: {:?}", err);
                Err(err)
            }
            Ok(v) => {
                info!("pool worker finished cleanly");
                Ok(v)
            }
        }
    });
    Ok((
        PoolCommandSender(command_tx),
        PoolManager {
            join_handle,
            manager_tx,
        },
    ))
}

#[derive(Clone)]
pub struct PoolCommandSender(pub mpsc::Sender<PoolCommand>);

impl PoolCommandSender {
    //TODO
}

pub struct PoolManager {
    join_handle: JoinHandle<Result<(), PoolError>>,
    manager_tx: mpsc::Sender<PoolManagementCommand>,
}

impl PoolManager {
    pub async fn stop(self) -> Result<(), PoolError> {
        massa_trace!("pool.pool_controller.stop", {});
        drop(self.manager_tx);
        self.join_handle.await.unwrap().unwrap();
        Ok(())
    }
}
