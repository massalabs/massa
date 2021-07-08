use super::{config::PoolConfig, error::PoolError};

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
pub enum PoolManagementCommand {}

/// Manages pool.
pub struct PoolWorker {
    controller_command_rx: mpsc::Receiver<PoolCommand>,
    /// Channel receiving pool management commands.
    controller_manager_rx: mpsc::Receiver<PoolManagementCommand>,
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
        controller_command_rx: mpsc::Receiver<PoolCommand>,
        controller_manager_rx: mpsc::Receiver<PoolManagementCommand>,
    ) -> Result<PoolWorker, PoolError> {
        massa_trace!("pool.pool_worker.new", {});
        Ok(PoolWorker {
            controller_command_rx,
            controller_manager_rx,
        })
    }

    /// Pool work is managed here.
    /// It's mostly a tokio::select within a loop.
    pub async fn run_loop(mut self) -> Result<(), PoolError> {
        loop {
            massa_trace!("pool.pool_worker.run_loop.select", {});
            tokio::select! {
                // listen pool commands
                Some(cmd) = self.controller_command_rx.recv() => {
                    massa_trace!("pool.pool_worker.run_loop.pool_command", {});
                    self.process_pool_command(cmd).await?
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
        Ok(())
    }

    /// Manages given pool command.
    ///
    /// # Argument
    /// * cmd: consens command to process
    async fn process_pool_command(&mut self, cmd: PoolCommand) -> Result<(), PoolError> {
        match cmd {
            //TODO
        }
    }
}
