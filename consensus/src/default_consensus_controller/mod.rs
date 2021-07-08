mod consensus_worker;
mod misc_collections;
use std::{collections::HashMap, net::IpAddr};

use crate::error::ConsensusError;
use communication::network::PeerInfo;

pub use consensus_worker::ConsensusCommand;
use crypto::signature::PublicKey;
use models::block::Block;

use super::{block_graph::*, config::ConsensusConfig, consensus_controller::*};
use async_trait::async_trait;
use communication::protocol::protocol_controller::ProtocolController;
use consensus_worker::ConsensusWorker;
use logging::debug;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;

/// Manages consensus.
#[derive(Debug)]
pub struct DefaultConsensusController<ProtocolControllerT: ProtocolController> {
    /// Consensus configuration.
    cfg: ConsensusConfig,
    /// Handle on the worker's task
    consensus_controller_handle: JoinHandle<()>,
    /// Channel to send out consensus commands.
    consensus_command_tx: Sender<ConsensusCommand>,
    /// Channel to receive consensus events.
    consensus_event_rx: Receiver<ConsensusEvent>,
    /// Phantom data marker - unused.
    _protocol_controller_t: std::marker::PhantomData<ProtocolControllerT>,
}

/// Interface to communicate with the controller.
#[derive(Debug, Clone)]
pub struct DefaultConsensusControllerInterface {
    /// Consensus configuration.
    cfg: ConsensusConfig,
    /// Channel to send out consensus commands.
    consensus_command_tx: Sender<ConsensusCommand>,
}

impl<ProtocolControllerT: ProtocolController + 'static>
    DefaultConsensusController<ProtocolControllerT>
{
    /// Creates a new consensus controller.
    ///
    /// # Arguments
    /// * cfg: consensus configuration
    /// * protocol_controller: associated protocol controller.
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

    /// Stops the consensus controller properly.
    pub async fn stop(mut self) -> Result<(), ConsensusError> {
        debug!("stopping consensus controller");
        massa_trace!("consensus_stop_begin", {});
        drop(self.consensus_command_tx);
        while let Some(_) = self.consensus_event_rx.recv().await {}
        self.consensus_controller_handle.await?;
        debug!("consensus controller stopped");
        massa_trace!("consensus_stop_end", {});
        Ok(())
    }
}

/// Manages consensus.
#[async_trait]
impl<ProtocolControllerT: ProtocolController> ConsensusController
    for DefaultConsensusController<ProtocolControllerT>
{
    type ProtocolControllerT = ProtocolControllerT;
    type ConsensusControllerInterfaceT = DefaultConsensusControllerInterface;

    /// Awaits for next consensus event.
    async fn wait_event(&mut self) -> Result<ConsensusEvent, ConsensusError> {
        self.consensus_event_rx
            .recv()
            .await
            .ok_or(ConsensusError::ControllerEventError)
    }

    /// Creates a new ConsensusControllerInterface, cloning required fields.
    fn get_interface(&self) -> Self::ConsensusControllerInterfaceT {
        DefaultConsensusControllerInterface {
            cfg: self.cfg.clone(),
            consensus_command_tx: self.consensus_command_tx.clone(),
        }
    }
}

#[async_trait]
impl ConsensusControllerInterface for DefaultConsensusControllerInterface {
    /// Gets all the aviable information on the block graph returning a Blockgraphexport.
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

    /// Gets the whole block corresponding to given hash.
    ///
    /// # Arguments
    /// * hash: hash corresponding to the block we want.
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

    /// Gets all known peers information.
    async fn get_peers(&self) -> Result<HashMap<std::net::IpAddr, PeerInfo>, ConsensusError> {
        let (response_tx, mut response_rx) = mpsc::channel::<HashMap<IpAddr, PeerInfo>>(1);
        self.consensus_command_tx
            .send(ConsensusCommand::GetPeers(response_tx))
            .await
            .map_err(|err| {
                ConsensusError::SendChannelError(format!("send error consensus command: {:?}", err))
            })?;
        response_rx
            .recv()
            .await
            .ok_or(ConsensusError::ReceiveChannelError("receive error".into()))
    }

    /// Gets (slot, public_key) were the staker with public_key was selected for slot, between start_slot and end_slot.
    ///
    /// # Arguments
    /// * start_slot: begining of the considered interval.
    /// * end_slot: end of the considered interval.
    async fn get_selection_draws(
        &self,
        start_slot: (u64, u8),
        end_slot: (u64, u8),
    ) -> Result<Vec<((u64, u8), PublicKey)>, ConsensusError> {
        let (response_tx, mut response_rx) =
            mpsc::channel::<Result<Vec<((u64, u8), PublicKey)>, ConsensusError>>(1);
        self.consensus_command_tx
            .send(ConsensusCommand::GetSelectionDraws(
                start_slot,
                end_slot,
                response_tx,
            ))
            .await
            .map_err(|err| {
                ConsensusError::SendChannelError(format!("send error consensus command: {:?}", err))
            })?;
        response_rx
            .recv()
            .await
            .ok_or(ConsensusError::ReceiveChannelError("receive error".into()))?
    }
}
