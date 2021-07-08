mod node_worker;
mod protocol_worker;
use super::config::ProtocolConfig;
pub use super::protocol_controller::ProtocolEvent;
use super::protocol_controller::{NodeId, ProtocolController};
use crate::error::{ChannelError, CommunicationError};
use crate::network::network_controller::NetworkController;
use async_trait::async_trait;
use crypto::{hash::Hash, signature::SignatureEngine};
use futures::StreamExt;
use models::block::Block;
pub use node_worker::{NodeCommand, NodeEvent};
pub use protocol_worker::ProtocolCommand;
use protocol_worker::ProtocolWorker;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;

#[derive(Debug)]
pub struct DefaultProtocolController<NetworkControllerT: NetworkController> {
    protocol_event_rx: Receiver<ProtocolEvent>,
    protocol_command_tx: Sender<ProtocolCommand>,
    protocol_controller_handle: JoinHandle<()>,
    _network_controller_t: std::marker::PhantomData<NetworkControllerT>,
}

impl<NetworkControllerT: NetworkController + 'static>
    DefaultProtocolController<NetworkControllerT>
{
    /// initiate a new ProtocolController from a ProtocolConfig
    /// - generate public / private key
    /// - create protocol_command/protocol_event channels
    /// - lauch protocol_controller_fn in an other task
    pub async fn new(cfg: ProtocolConfig, network_controller: NetworkControllerT) -> Self {
        debug!("starting protocol controller");
        massa_trace!("start", {});

        // generate our own random PublicKey (and therefore NodeId) and keep private key
        let signature_engine = SignatureEngine::new();
        let private_key = SignatureEngine::generate_random_private_key();
        let self_node_id = NodeId(signature_engine.derive_public_key(&private_key));

        debug!("local protocol node_id={:?}", self_node_id);
        massa_trace!("self_node_id", { "node_id": self_node_id });

        // launch worker
        let (protocol_event_tx, protocol_event_rx) = mpsc::channel::<ProtocolEvent>(1024);
        let (protocol_command_tx, protocol_command_rx) = mpsc::channel::<ProtocolCommand>(1024);
        let protocol_controller_handle = tokio::spawn(async move {
            ProtocolWorker::new(
                cfg,
                self_node_id,
                private_key,
                network_controller,
                protocol_event_tx,
                protocol_command_rx,
            )
            .run_loop()
            .await
            .expect("ProtocolWorker loop crash"); //in a spawned task
        });

        debug!("protocol controller ready");
        massa_trace!("ready", {});

        DefaultProtocolController::<NetworkControllerT> {
            protocol_event_rx,
            protocol_command_tx,
            protocol_controller_handle,
            _network_controller_t: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<NetworkControllerT: NetworkController> ProtocolController
    for DefaultProtocolController<NetworkControllerT>
{
    type NetworkControllerT = NetworkControllerT;

    /// Receives the next ProtocolEvent from connected Node.
    /// None is returned when all Sender halves have dropped,
    /// indicating that no further values can be sent on the channel
    /// panics if the protocol controller event can't be retrieved
    async fn wait_event(&mut self) -> Result<ProtocolEvent, CommunicationError> {
        self.protocol_event_rx.recv().await.ok_or(
            ChannelError::ReceiveProtocolEventError(
                "DefaultProtocolController wait_event channel dropped".to_string(),
            )
            .into(),
        )
    }

    /// Stop the protocol controller
    /// panices if the protocol controller is not reachable
    async fn stop(mut self) -> Result<(), CommunicationError> {
        debug!("stopping protocol controller");
        massa_trace!("begin", {});
        drop(self.protocol_command_tx);
        while let Some(_) = self.protocol_event_rx.next().await {}
        self.protocol_controller_handle.await?;
        debug!("protocol controller stopped");
        massa_trace!("end", {});
        Ok(())
    }

    async fn propagate_block(
        &mut self,
        hash: Hash,
        block: &Block,
    ) -> Result<(), CommunicationError> {
        massa_trace!("block_propagation_order", { "block": hash });
        self.protocol_command_tx
            .send(ProtocolCommand::PropagateBlock {
                hash,
                block: block.clone(),
            })
            .await
            .map_err(|err| ChannelError::from(err).into())
    }
}
