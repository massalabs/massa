use super::{
    config::{PoolConfig, CHANNEL_SIZE},
    pool_worker::{
        PoolCommand, PoolManagementCommand, PoolWorker,
    },
    error::PoolError,
};
use tokio::{
    task::JoinHandle,
    sync::{mpsc, oneshot},
    time::{sleep_until, Sleep},
};
use communication::protocol::{ProtocolCommandSender, ProtocolPoolEventReceiver};
use models::{SerializationContext};
use logging::{debug, massa_trace};


/// Creates a new pool controller.
///
/// # Arguments
/// * cfg: pool configuration
/// * protocol_command_sender: a ProtocolCommandSender instance to send commands to Protocol.
/// * protocol_pool_event_receiver: a ProtocolPoolEventReceiver instance to receive pool events from Protocol.
pub async fn start_pool_controller(
    cfg: PoolConfig,
    serialization_context: SerializationContext,
    protocol_command_sender: ProtocolCommandSender,
    protocol_pool_event_receiver: ProtocolPoolEventReceiver,
    clock_compensation: i64,
) -> Result<
    (
        PoolCommandSender,
        PoolManager,
    ),
    PoolError,
> {
    debug!("starting pool controller");
    massa_trace!(
        "pool.pool_controller.start_pool_controller",
        {}
    );

    // start worker
    let (command_tx, command_rx) = mpsc::channel::<PoolCommand>(CHANNEL_SIZE);
    let (manager_tx, manager_rx) = mpsc::channel::<PoolManagementCommand>(1);
    let cfg_copy = cfg.clone();
    let join_handle = tokio::spawn(async move {
        let res = PoolWorker::new(
            cfg_copy,
            protocol_command_sender,
            protocol_pool_event_receiver,
            command_rx,
            manager_rx,
            clock_compensation,
        )?
        .run_loop()
        .await;
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
    join_handle: JoinHandle<Result<ProtocolPoolEventReceiver, PoolError>>,
    manager_tx: mpsc::Sender<PoolManagementCommand>,
}

impl PoolManager {
    pub async fn stop(self) -> Result<(), PoolError> {
        massa_trace!("pool.pool_controller.stop", {});
        drop(self.manager_tx);
        let protocol_pool_event_receiver = self.join_handle.await??;
        Ok(protocol_pool_event_receiver)
    }
}
