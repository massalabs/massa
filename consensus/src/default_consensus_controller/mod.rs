mod consensus_worker;
mod misc_collections;
use std::{collections::HashMap, net::IpAddr};

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

#[derive(Debug)]
pub struct DefaultConsensusController<ProtocolControllerT: ProtocolController> {
    cfg: ConsensusConfig,
    consensus_controller_handle: JoinHandle<()>,
    consensus_command_tx: Sender<ConsensusCommand>,
    consensus_event_rx: Receiver<ConsensusEvent>,
    _protocol_controller_t: std::marker::PhantomData<ProtocolControllerT>,
}

#[derive(Debug, Clone)]
pub struct DefaultConsensusControllerInterface {
    cfg: ConsensusConfig,
    consensus_command_tx: Sender<ConsensusCommand>,
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
            cfg: cfg.clone(),
            consensus_controller_handle,
            consensus_command_tx,
            consensus_event_rx,
            _protocol_controller_t: std::marker::PhantomData,
        })
    }

    /// Stop the consensus controller
    /// panices if the consensus controller is not reachable
    async fn stop(mut self) -> Result<(), ConsensusError> {
        debug!("stopping consensus controller");
        massa_trace!("begin", {});
        while let Some(_) = self.consensus_event_rx.recv().await {}
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
    type ConsensusControllerInterfaceT = DefaultConsensusControllerInterface;

    async fn wait_event(&mut self) -> Result<ConsensusEvent, ConsensusError> {
        self.consensus_event_rx
            .recv()
            .await
            .ok_or(ConsensusError::ControllerEventError)
    }

    fn get_interface(&self) -> Self::ConsensusControllerInterfaceT {
        DefaultConsensusControllerInterface {
            cfg: self.cfg.clone(),
            consensus_command_tx: self.consensus_command_tx.clone(),
        }
    }
}

#[async_trait]
impl ConsensusControllerInterface for DefaultConsensusControllerInterface {
    async fn get_block_graph_status(&self) -> Result<BlockGraphExport, ConsensusError> {
        let (response_tx, mut response_rx) = mpsc::channel::<BlockGraphExport>(1);
        self.consensus_command_tx
            .send(ConsensusCommand::GetBlockGraphStatus(response_tx))
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(format!("send error consensus command"))
            })?;
        response_rx
            .recv()
            .await
            .ok_or(ConsensusError::ReceiveChannelError(format!(
                "receive error"
            )))
    }

    async fn get_active_block(
        &self,
        hash: crypto::hash::Hash,
    ) -> Result<Option<Block>, ConsensusError> {
        let (response_tx, mut response_rx) = mpsc::channel::<Option<Block>>(1);
        self.consensus_command_tx
            .send(ConsensusCommand::GetActiveBlock(hash, response_tx))
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(format!("send error consensus command"))
            })?;
        response_rx
            .recv()
            .await
            .ok_or(ConsensusError::ReceiveChannelError(format!(
                "receive error"
            )))
    }

    async fn get_peers(
        &self,
    ) -> Result<std::collections::HashMap<std::net::IpAddr, String>, ConsensusError> {
        let (response_tx, mut response_rx) = mpsc::channel::<HashMap<IpAddr, String>>(1);
        self.consensus_command_tx
            .send(ConsensusCommand::GetPeers(response_tx))
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(format!("send error consensus command"))
            })?;
        response_rx
            .recv()
            .await
            .ok_or(ConsensusError::ReceiveChannelError(format!(
                "receive error"
            )))
    }
}
