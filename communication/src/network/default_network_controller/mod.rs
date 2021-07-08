mod network_worker;

use super::config::NetworkConfig;
use super::establisher::Establisher;
use super::network_controller::*;
use super::peer_info_database::*;
use async_trait::async_trait;
use futures::StreamExt;
use network_worker::{NetworkCommand, NetworkWorker};
use std::error::Error;
use std::net::IpAddr;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

type BoxResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Debug)]
pub struct DefaultNetworkController<EstablisherT: Establisher> {
    network_command_tx: mpsc::Sender<NetworkCommand>,
    network_event_rx: mpsc::Receiver<NetworkEvent<EstablisherT::ReaderT, EstablisherT::WriterT>>,
    controller_fn_handle: JoinHandle<()>,
}

impl<EstablisherT: Establisher + 'static> DefaultNetworkController<EstablisherT> {
    /// Starts a new NetworkController from NetworkConfig
    /// can panic if :
    /// - config routable_ip IP is not routable
    pub async fn new(cfg: &NetworkConfig, mut establisher: EstablisherT) -> BoxResult<Self> {
        debug!("starting network controller");
        massa_trace!("start", {});

        // check that local IP is routable
        if let Some(self_ip) = cfg.routable_ip {
            if !self_ip.is_global() {
                panic!("config routable_ip IP is not routable");
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
    type EstablisherT = EstablisherT;
    type ReaderT = EstablisherT::ReaderT;
    type WriterT = EstablisherT::WriterT;

    /// Stops the NetworkController properly
    /// can panic if network controller is not reachable
    async fn stop(mut self) {
        debug!("stopping network controller");
        massa_trace!("begin", {});
        drop(self.network_command_tx);
        while let Some(_) = self.network_event_rx.next().await {}
        self.controller_fn_handle
            .await
            .expect("failed joining network controller");
        debug!("network controller stopped");
        massa_trace!("end", {});
    }

    async fn wait_event(&mut self) -> NetworkEvent<EstablisherT::ReaderT, EstablisherT::WriterT> {
        self.network_event_rx
            .recv()
            .await
            .expect("failed retrieving network controller event")
    }

    async fn merge_advertised_peer_list(&mut self, ips: Vec<IpAddr>) {
        self.network_command_tx
            .send(NetworkCommand::MergeAdvertisedPeerList(ips))
            .await
            .expect("network controller disappeared");
    }

    async fn get_advertisable_peer_list(&mut self) -> Vec<IpAddr> {
        let (response_tx, response_rx) = oneshot::channel::<Vec<IpAddr>>();
        self.network_command_tx
            .send(NetworkCommand::GetAdvertisablePeerList(response_tx))
            .await
            .expect("network controller disappeared");
        response_rx.await.expect("network controller disappeared")
    }

    async fn connection_closed(&mut self, id: ConnectionId, reason: ConnectionClosureReason) {
        self.network_command_tx
            .send(NetworkCommand::ConnectionClosed((id, reason)))
            .await
            .expect("network controller disappeared");
    }

    async fn connection_alive(&mut self, id: ConnectionId) {
        self.network_command_tx
            .send(NetworkCommand::ConnectionAlive(id))
            .await
            .expect("network controller disappeared");
    }
}
