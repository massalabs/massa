mod consensus_worker;
use super::{block_database::*, config::ConsensusConfig, consensus_controller::*};
use crate::logging::debug;
use crate::protocol::protocol_controller::ProtocolController;
use async_trait::async_trait;
use consensus_worker::{ConsensusCommand, ConsensusWorker};
use std::error::Error;
use tokio::stream::StreamExt;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;

type BoxResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

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
    ) -> BoxResult<Self> {
        debug!("starting consensus controller");
        massa_trace!("start", {});

        let block_db = BlockDatabase::new();
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
            .run_loop()
            .await
        });

        Ok(DefaultConsensusController::<ProtocolControllerT> {
            consensus_controller_handle,
            consensus_command_tx,
            consensus_event_rx,
            _protocol_controller_t: std::marker::PhantomData,
        })
    }

    /// Stop the consensus controller
    /// panices if the consensus controller is not reachable
    async fn stop(mut self) {
        debug!("stopping consensus controller");
        massa_trace!("begin", {});
        drop(self.consensus_command_tx);
        while let Some(_) = self.consensus_event_rx.next().await {}
        self.consensus_controller_handle
            .await
            .expect("failed joining consensus controller");
        debug!("consensus controller stopped");
        massa_trace!("end", {});
    }
}

#[async_trait]
impl<ProtocolControllerT: ProtocolController> ConsensusController
    for DefaultConsensusController<ProtocolControllerT>
{
    type ProtocolControllerT = ProtocolControllerT;

    async fn wait_event(&mut self) -> ConsensusEvent {
        self.consensus_event_rx
            .recv()
            .await
            .expect("failed retrieving consensus controller event")
    }

    async fn generate_random_block(&self) {
        use rand::distributions::Alphanumeric;
        use rand::Rng;

        let block = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .collect::<String>();
        self.consensus_command_tx
            .send(ConsensusCommand::CreateBlock(block))
            .await
            .expect("could not send consensus command");
    }
}
