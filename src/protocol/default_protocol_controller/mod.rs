mod node_worker;
mod protocol_worker;
use super::{config::ProtocolConfig, protocol_controller::*};
use crate::crypto::signature::SignatureEngine;
use crate::logging::debug;
use crate::network::network_controller::NetworkController;
use crate::structures::block::Block;
use async_trait::async_trait;
use futures::StreamExt;
use protocol_worker::{ProtocolCommand, ProtocolWorker};
use std::error::Error;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;

type BoxResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

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
    /// returns the ProtocolController in a BoxResult once it is ready
    pub async fn new(
        cfg: ProtocolConfig,
        network_controller: NetworkControllerT,
    ) -> BoxResult<Self> {
        debug!("starting protocol controller");
        massa_trace!("start", {});

        // generate our own random PublicKey (and therefore NodeId) and keep private key
        let private_key;
        let self_node_id;
        {
            let signature_engine = SignatureEngine::new();
            private_key =
                SignatureEngine::generate_random_private_key(&mut SignatureEngine::create_rng());
            self_node_id = NodeId(signature_engine.derive_public_key(&private_key));
        }
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
            .await;
        });

        debug!("protocol controller ready");
        massa_trace!("ready", {});

        Ok(DefaultProtocolController::<NetworkControllerT> {
            protocol_event_rx,
            protocol_command_tx,
            protocol_controller_handle,
            _network_controller_t: std::marker::PhantomData,
        })
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
    async fn wait_event(&mut self) -> ProtocolEvent {
        self.protocol_event_rx
            .recv()
            .await
            .expect("failed retrieving protocol controller event")
    }

    /// Stop the protocol controller
    /// panices if the protocol controller is not reachable
    async fn stop(mut self) {
        debug!("stopping protocol controller");
        massa_trace!("begin", {});
        drop(self.protocol_command_tx);
        while let Some(_) = self.protocol_event_rx.next().await {}
        self.protocol_controller_handle
            .await
            .expect("failed joining protocol controller");
        debug!("protocol controller stopped");
        massa_trace!("end", {});
    }

    async fn propagate_block(
        &mut self,
        block: &Block,
        exclude_node: Option<NodeId>,
        restrict_to_node: Option<NodeId>,
    ) {
        self.protocol_command_tx
            .send(ProtocolCommand::PropagateBlock {
                block: block.clone(),
                exclude_node,
                restrict_to_node,
            })
            .await
            .expect("protcol_command channel disappeared");
    }
}
