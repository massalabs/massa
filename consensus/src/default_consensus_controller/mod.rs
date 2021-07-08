mod consensus_worker;
mod misc_collections;
use crate::error::ConsensusError;

pub use consensus_worker::ConsensusCommand;
use models::block::Block;

use super::{block_graph::*, config::ConsensusConfig, consensus_controller::*};
use async_trait::async_trait;
use communication::protocol::protocol_controller::ProtocolController;
use consensus_worker::ConsensusWorker;
use logging::debug;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;
use tokio::{stream::StreamExt, sync::oneshot};

#[derive(Debug)]
pub struct DefaultConsensusController<ProtocolControllerT: ProtocolController> {
    consensus_controller_handle: JoinHandle<()>,
    consensus_command_tx: Sender<ConsensusCommand>,
    consensus_event_rx: Receiver<ConsensusEvent>,
    _protocol_controller_t: std::marker::PhantomData<ProtocolControllerT>,
}

impl<ProtocolControllerT: ProtocolController + 'static>
    DefaultConsensusController<ProtocolControllerT>
{
    pub async fn new(
        cfg: &ConsensusConfig,
        protocol_controller: ProtocolControllerT,
    ) -> Result<Self, ConsensusError> {
        debug!("starting consensus controller");
        massa_trace!("start", {});

        // ensure that the parameters are sane
        if cfg.thread_count == 0 {
            return Err(ConsensusError::ConfigError(format!(
                "thread_count shoud be strictly more than 0"
            )));
        }
        if cfg.t0 == 0.into() {
            return Err(ConsensusError::ConfigError(format!(
                "t0 shoud be strictly more than 0"
            )));
        }
        if cfg.t0.checked_rem_u64(cfg.thread_count as u64)? != 0.into() {
            return Err(ConsensusError::ConfigError(format!(
                "thread_count should divide t0"
            )));
        }

        // start worker
        let block_db = BlockGraph::new(cfg)?;
        let (consensus_command_tx, consensus_command_rx) = mpsc::channel::<ConsensusCommand>(1024);
        let (consensus_event_tx, consensus_event_rx) = mpsc::channel::<ConsensusEvent>(1024);
        let cfg_copy = cfg.clone();
        let consensus_controller_handle = tokio::spawn(async move {
            ConsensusWorker::new(
                cfg_copy,
                protocol_controller,
                block_db,
                consensus_command_rx,
                consensus_event_tx,
            )
            .expect("Could not start consensus worker") // in a spawned task
            .run_loop()
            .await
            .expect("error while running consensus worker") // in a spawned task
        });

        Ok(DefaultConsensusController::<ProtocolControllerT> {
            consensus_controller_handle,
            consensus_command_tx,
            consensus_event_rx,
            _protocol_controller_t: std::marker::PhantomData,
        })
    }

    pub async fn send_consensus_command(
        &self,
        command: ConsensusCommand,
    ) -> Result<(), ConsensusError> {
        Ok(self
            .consensus_command_tx
            .send(command)
            .await
            .map_err(|err| {
                ConsensusError::SendChannelError(format!(
                    "could not send GetBlockGraphStatus answer:{:?}",
                    err
                ))
            })?)
    }

    /// Stop the consensus controller
    /// panices if the consensus controller is not reachable
    async fn stop(mut self) -> Result<(), ConsensusError> {
        debug!("stopping consensus controller");
        massa_trace!("begin", {});
        drop(self.consensus_command_tx);
        while let Some(_) = self.consensus_event_rx.next().await {}
        self.consensus_controller_handle.await?;
        debug!("consensus controller stopped");
        massa_trace!("end", {});
        Ok(())
    }
}

#[async_trait]
impl<ProtocolControllerT: ProtocolController> ConsensusController
    for DefaultConsensusController<ProtocolControllerT>
{
    type ProtocolControllerT = ProtocolControllerT;

    async fn wait_event(&mut self) -> Result<ConsensusEvent, ConsensusError> {
        self.consensus_event_rx
            .recv()
            .await
            .ok_or(ConsensusError::ControllerEventError)
    }

    async fn get_block_graph_status(&self) -> Result<BlockGraphExport, ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel::<BlockGraphExport>();
        self.consensus_command_tx
            .send(ConsensusCommand::GetBlockGraphStatus(response_tx))
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(format!("send error consensus command"))
            })?;
        Ok(response_rx
            .await
            .map_err(|_| ConsensusError::ReceiveChannelError(format!("receive error")))?)
    }

    async fn get_active_block(
        &self,
        hash: crypto::hash::Hash,
    ) -> Result<Option<Block>, ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel::<Option<Block>>();
        self.consensus_command_tx
            .send(ConsensusCommand::GetActiveBlock(hash, response_tx))
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(format!("send error consensus command"))
            })?;
        Ok(response_rx
            .await
            .map_err(|_| ConsensusError::ReceiveChannelError(format!("receive error")))?)
    }
}
