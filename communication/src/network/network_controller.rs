use super::{
    common::{ConnectionClosureReason, ConnectionId},
    config::{NetworkConfig, CHANNEL_SIZE},
    establisher::Establisher,
    network_worker::{NetworkCommand, NetworkEvent, NetworkManagementCommand, NetworkWorker},
    peer_info_database::*,
};
use crate::error::CommunicationError;
use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

/// Starts a new NetworkWorker in a spawned task
///
/// # Arguments
/// * cfg : network configuration
pub async fn start_network_controller(
    cfg: NetworkConfig,
    mut establisher: Establisher,
) -> Result<(NetworkCommandSender, NetworkEventReceiver, NetworkManager), CommunicationError> {
    debug!("starting network controller");

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
    let (command_tx, command_rx) = mpsc::channel::<NetworkCommand>(CHANNEL_SIZE);
    let (event_tx, event_rx) = mpsc::channel::<NetworkEvent>(CHANNEL_SIZE);
    let (manager_tx, manager_rx) = mpsc::channel::<NetworkManagementCommand>(1);
    let cfg_copy = cfg.clone();
    let join_handle = tokio::spawn(async move {
        NetworkWorker::new(
            cfg_copy,
            listener,
            establisher,
            peer_info_db,
            command_rx,
            event_tx,
            manager_rx,
        )
        .run_loop()
        .await
    });

    debug!("network controller started");

    Ok((
        NetworkCommandSender(command_tx),
        NetworkEventReceiver(event_rx),
        NetworkManager {
            join_handle,
            manager_tx,
        },
    ))
}

#[derive(Clone)]
pub struct NetworkCommandSender(pub mpsc::Sender<NetworkCommand>);

impl NetworkCommandSender {
    /// Transmit the order to merge the peer list.
    ///
    /// # Argument
    /// ips : vec of advertized ips
    pub async fn merge_advertised_peer_list(
        &mut self,
        ips: Vec<IpAddr>,
    ) -> Result<(), CommunicationError> {
        self.0
            .send(NetworkCommand::MergeAdvertisedPeerList(ips))
            .await
            .map_err(|_| {
                CommunicationError::ChannelError(
                    "cound not send MergeAdvertisedPeerList command".into(),
                )
            })?;
        Ok(())
    }

    /// Send the order to get advertisable peer list.
    pub async fn get_advertisable_peer_list(&mut self) -> Result<Vec<IpAddr>, CommunicationError> {
        let (response_tx, response_rx) = oneshot::channel::<Vec<IpAddr>>();
        self.0
            .send(NetworkCommand::GetAdvertisablePeerList(response_tx))
            .await
            .map_err(|_| {
                CommunicationError::ChannelError(
                    "cound not send GetAdvertisablePeerList command".into(),
                )
            })?;
        Ok(response_rx.await.map_err(|_| {
            CommunicationError::ChannelError(
                "could not send GetAdvertisablePeerList upstream".into(),
            )
        })?)
    }

    /// Send the order to get peers.
    pub async fn get_peers(&mut self) -> Result<HashMap<IpAddr, PeerInfo>, CommunicationError> {
        let (response_tx, response_rx) = oneshot::channel::<HashMap<IpAddr, PeerInfo>>();
        self.0
            .send(NetworkCommand::GetPeers(response_tx))
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("cound not send GetPeers command".into())
            })?;
        Ok(response_rx.await.map_err(|_| {
            CommunicationError::ChannelError(
                "could not send GetAdvertisablePeerListChannelError upstream".into(),
            )
        })?)
    }

    /// Send the information that the connection has been closed for given reason.
    ///
    /// # Arguments
    /// * id : connenction id of the closed connection
    /// * reason : connection closure reason
    pub async fn connection_closed(
        &mut self,
        id: ConnectionId,
        reason: ConnectionClosureReason,
    ) -> Result<(), CommunicationError> {
        self.0
            .send(NetworkCommand::ConnectionClosed((id, reason)))
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("cound not send ConnectionClosed command".into())
            })?;
        Ok(())
    }

    /// Send the information that the connection is alive.
    ///
    /// # Arguments
    /// * id : connenction id
    pub async fn connection_alive(&mut self, id: ConnectionId) -> Result<(), CommunicationError> {
        self.0
            .send(NetworkCommand::ConnectionAlive(id))
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("cound not send ConnectionAlive command".into())
            })?;
        Ok(())
    }
}

pub struct NetworkEventReceiver(pub mpsc::Receiver<NetworkEvent>);

impl NetworkEventReceiver {
    pub async fn wait_event(&mut self) -> Result<NetworkEvent, CommunicationError> {
        self.0.recv().await.ok_or(CommunicationError::ChannelError(
            "could not receive event".into(),
        ))
    }

    /// drains remaining events and returns them in a VecDeque
    /// note: events are sorted from oldest to newest
    pub async fn drain(mut self) -> VecDeque<NetworkEvent> {
        let mut remaining_events: VecDeque<NetworkEvent> = VecDeque::new();
        while let Some(evt) = self.0.recv().await {
            remaining_events.push_back(evt);
        }
        remaining_events
    }
}

pub struct NetworkManager {
    join_handle: JoinHandle<Result<(), CommunicationError>>,
    manager_tx: mpsc::Sender<NetworkManagementCommand>,
}

impl NetworkManager {
    pub async fn stop(
        self,
        network_event_receiver: NetworkEventReceiver,
    ) -> Result<(), CommunicationError> {
        drop(self.manager_tx);
        let _remaining_events = network_event_receiver.drain().await;
        let _ = self.join_handle.await?;
        Ok(())
    }
}
