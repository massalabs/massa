use super::{config::PoolConfig, error::PoolError};
use communication::protocol::{ProtocolCommandSender, ProtocolPoolEvent, ProtocolPoolEventReceiver};

use tokio::{
    sync::{mpsc, oneshot},
    time::{sleep_until, Sleep},
};

/// Commands that can be proccessed by pool.
#[derive(Debug)]
pub enum PoolCommand {
    //TODO
}

/// Events that are emitted by pool.
#[derive(Debug, Clone)]
pub enum PoolManagementCommand {
}

/// Manages pool.
pub struct PoolWorker {
    /// Consensus Configuration
    cfg: PoolConfig,
    /// Associated protocol command sender.
    protocol_command_sender: ProtocolCommandSender,
    /// Associated protocol pool event listener.
    protocol_pool_event_receiver: ProtocolPoolEventReceiver,
    /// Channel receiving pool commands.
    controller_command_rx: mpsc::Receiver<PoolCommand>,
    /// Channel receiving pool management commands.
    controller_manager_rx: mpsc::Receiver<PoolManagementCommand>,
    /// Clock compensation
    clock_compensation: i64,
}

impl PoolWorker {
    /// Creates a new pool controller.
    /// Initiates the random selector.
    ///
    /// # Arguments
    /// * cfg: pool configuration.
    /// * protocol_command_sender: associated protocol controller
    /// * protocol_command_sender protocol pool event receiver
    /// * controller_command_rx: Channel receiving pool commands.
    /// * controller_manager_rx: Channel receiving pool management commands.
    pub fn new(
        cfg: PoolConfig,
        protocol_command_sender: ProtocolCommandSender,
        protocol_pool_event_receiver: ProtocolPoolEventReceiver,
        controller_command_rx: mpsc::Receiver<PoolCommand>,
        controller_manager_rx: mpsc::Receiver<PoolManagementCommand>,
        clock_compensation: i64,
    ) -> Result<PoolWorker, PoolError> {
        massa_trace!("pool.pool_worker.new", {});
        Ok(PoolWorker {
            cfg: cfg.clone(),
            protocol_command_sender,
            protocol_pool_event_receiver,
            controller_command_rx,
            controller_manager_rx,
            clock_compensation,
        })
    }

    /// Pool work is managed here.
    /// It's mostly a tokio::select within a loop.
    pub async fn run_loop(mut self) -> Result<ProtocolPoolEventReceiver, PoolError> {
        loop {
            massa_trace!("pool.pool_worker.run_loop.select", {});
            tokio::select! {
                // listen pool commands
                Some(cmd) = self.controller_command_rx.recv() => {
                    massa_trace!("pool.pool_worker.run_loop.pool_command", {});
                    self.process_pool_command(cmd).await?
                },
                // receive protocol controller pool events
                evt = self.protocol_pool_event_receiver.wait_event() => {
                    massa_trace!("pool.pool_worker.run_loop.select.protocol_event", {});
                    match evt {
                        Ok(event) => self.process_protocol_pool_event(event).await?,
                        Err(err) => return Err(PoolError::CommunicationError(err))
                    }
                },
                // listen to manager commands
                cmd = self.controller_manager_rx.recv() => {
                    massa_trace!("pool.pool_worker.run_loop.select.manager", {});
                    match cmd {
                    None => break,
                    Some(_) => {}
                }}
            }
        }
        // end loop
        Ok(self.protocol_pool_event_receiver)
    }

    /// Manages given pool command.
    ///
    /// # Argument
    /// * cmd: consens command to process
    async fn process_pool_command(
        &mut self,
        cmd: PoolCommand,
    ) -> Result<(), PoolError> {
        match cmd {
            //TODO
        }
    }

    /// Manages received protocol pool events.
    ///
    /// # Arguments
    /// * event: event type to process.
    async fn process_protocol_pool_event(&mut self, event: ProtocolPoolEvent) -> Result<(), PoolError> {
        match event {
            //TODO
        }
        Ok(())
    }
}
