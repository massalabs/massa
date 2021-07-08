/// default_network_controller is the used implementation of NetworkController
mod network_worker;

use crate::error::{ChannelError, CommunicationError};

use super::config::NetworkConfig;
use super::establisher::Establisher;
use super::network_controller::*;
use super::peer_info_database::*;
use async_trait::async_trait;
pub use network_worker::{NetworkCommand, NetworkWorker};
use std::{collections::HashMap, net::IpAddr};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

/// Manages connections with a node.
/// For the most, it does nothing but sending orders to the worker.
#[derive(Debug)]
pub struct DefaultNetworkController<EstablisherT: Establisher> {
    /// channel used fo send network commands to the worker
    network_command_tx: mpsc::Sender<NetworkCommand>,
    /// channel used to receive network events from the worker
    network_event_rx: mpsc::Receiver<NetworkEvent<EstablisherT::ReaderT, EstablisherT::WriterT>>,
    /// handle used to close the worker's spawned task
    controller_fn_handle: JoinHandle<()>,
}

impl<EstablisherT: Establisher + 'static> DefaultNetworkController<EstablisherT> {
    /// Starts a new NetworkController in a spawned task
    ///
    /// # Arguments
    /// * cfg : network configuration
    /// * establisher : connection establisher
    pub async fn new(
        cfg: &NetworkConfig,
        mut establisher: EstablisherT,
    ) -> Result<Self, CommunicationError> {
        debug!("starting network controller");
        massa_trace!("start", {});

        // check that local IP is routable
        if let Some(self_ip) = cfg.routable_ip {
            if !self_ip.is_global() {
                return Err(CommunicationError::InvalidIpError(self_ip));
            }
        }

        // create listener
        let listener = establisher.get_listener(cfg.bind).await?;

        // load peer info database
        let peer_info_db = PeerInfoDatabase::new(&cfg).await?;

        // launch controller
        let (network_command_tx, network_command_rx) = mpsc::channel::<NetworkCommand>(1024);
        let (network_event_tx, network_event_rx) =
            mpsc::channel::<NetworkEvent<EstablisherT::ReaderT, EstablisherT::WriterT>>(1024);
        let cfg_copy = cfg.clone();
        let controller_fn_handle = tokio::spawn(async move {
            NetworkWorker::new(
                cfg_copy,
                listener,
                establisher,
                peer_info_db,
                network_command_rx,
                network_event_tx,
            )
            .run_loop()
            .await
            .expect("Error while running networkworker loop") // in a spawned task
        });

        debug!("network controller started");
        massa_trace!("ready", {});

        Ok(DefaultNetworkController::<EstablisherT> {
            network_command_tx,
            network_event_rx,
            controller_fn_handle,
        })
    }
}

#[async_trait]
impl<EstablisherT: Establisher> NetworkController for DefaultNetworkController<EstablisherT> {
    /// Establish a connection with a peer.
    type EstablisherT = EstablisherT;
    /// Reader part of the establisher.
    type ReaderT = EstablisherT::ReaderT;
    /// Writer part of the establisher.
    type WriterT = EstablisherT::WriterT;

    /// Used to close properly everything down.
    async fn stop(mut self) -> Result<(), CommunicationError> {
        debug!("stopping network controller");
        massa_trace!("begin", {});
        drop(self.network_command_tx);
        while let Some(_) = self.network_event_rx.recv().await {}
        self.controller_fn_handle.await?;
        debug!("network controller stopped");
        massa_trace!("end", {});
        Ok(())
    }

    /// Used to listen network events.
    async fn wait_event(
        &mut self,
    ) -> Result<NetworkEvent<EstablisherT::ReaderT, EstablisherT::WriterT>, CommunicationError>
    {
        self.network_event_rx
            .recv()
            .await
            .ok_or(ChannelError::NetworkControllerEventError.into())
    }

    /// Transmit the order to merge the peer list.
    ///
    /// # Argument
    /// ips : vec of advertized ips
    async fn merge_advertised_peer_list(
        &mut self,
        ips: Vec<IpAddr>,
    ) -> Result<(), CommunicationError> {
        self.network_command_tx
            .send(NetworkCommand::MergeAdvertisedPeerList(ips))
            .await
            .map_err(|err| ChannelError::from(err))?;
        Ok(())
    }

    /// Send the order to get advertisable peer list.
    async fn get_advertisable_peer_list(&mut self) -> Result<Vec<IpAddr>, CommunicationError> {
        let (response_tx, response_rx) = oneshot::channel::<Vec<IpAddr>>();
        self.network_command_tx
            .send(NetworkCommand::GetAdvertisablePeerList(response_tx))
            .await
            .map_err(|err| ChannelError::from(err))?;
        Ok(response_rx.await.map_err(|err| ChannelError::from(err))?)
    }

    /// Send the order to get peers.
    async fn get_peers(&mut self) -> Result<HashMap<IpAddr, PeerInfo>, CommunicationError> {
        let (response_tx, response_rx) = oneshot::channel::<HashMap<IpAddr, PeerInfo>>();
        self.network_command_tx
            .send(NetworkCommand::GetPeers(response_tx))
            .await
            .map_err(|err| ChannelError::from(err))?;
        Ok(response_rx.await.map_err(|err| ChannelError::from(err))?)
    }

    /// Send the information that the connection has been closed for given reason.
    ///
    /// # Arguments
    /// * id : connenction id of the closed connection
    /// * reason : connection closure reason
    async fn connection_closed(
        &mut self,
        id: ConnectionId,
        reason: ConnectionClosureReason,
    ) -> Result<(), CommunicationError> {
        self.network_command_tx
            .send(NetworkCommand::ConnectionClosed((id, reason)))
            .await
            .map_err(|err| ChannelError::from(err))?;
        Ok(())
    }

    /// Send the information that the connection is alive.
    ///
    /// # Arguments
    /// * id : connenction id
    async fn connection_alive(&mut self, id: ConnectionId) -> Result<(), CommunicationError> {
        self.network_command_tx
            .send(NetworkCommand::ConnectionAlive(id))
            .await
            .map_err(|err| ChannelError::from(err))?;
        Ok(())
    }
}
