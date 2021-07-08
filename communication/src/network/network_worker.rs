//! The network worker actually does the job of managing connections
//! That's why it's ... a worker ! 🦀
use super::{
    common::{ConnectionClosureReason, ConnectionId},
    config::NetworkConfig,
    establisher::{Establisher, Listener, ReadHalf, WriteHalf},
    peer_info_database::*,
};
use crate::error::CommunicationError;
use crate::logging::debug;
use futures::{stream::FuturesUnordered, StreamExt};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
};
use tokio::sync::{mpsc, oneshot};

/// Commands that the worker can execute
#[derive(Debug)]
pub enum NetworkCommand {
    MergeAdvertisedPeerList(Vec<IpAddr>),
    GetAdvertisablePeerList(oneshot::Sender<Vec<IpAddr>>),
    ConnectionClosed((ConnectionId, ConnectionClosureReason)),
    ConnectionAlive(ConnectionId),
    GetPeers(oneshot::Sender<HashMap<IpAddr, PeerInfo>>),
}

#[derive(Debug)]
pub enum NetworkEvent {
    NewConnection((ConnectionId, ReadHalf, WriteHalf)),
    ConnectionBanned(ConnectionId),
}

#[derive(Debug)]
pub enum NetworkManagementCommand {}

/// Real job is done by network worker
pub struct NetworkWorker {
    /// Network configuration.
    cfg: NetworkConfig,
    /// Listener part of the establisher.
    listener: Listener,
    /// The connection establisher.
    establisher: Establisher,
    /// Database with peer information.
    peer_info_db: PeerInfoDatabase,
    /// Receiver for network commands
    controller_command_rx: mpsc::Receiver<NetworkCommand>,
    /// Sender for network events
    controller_event_tx: mpsc::Sender<NetworkEvent>,
    /// Receiver for network management commands
    controller_manager_rx: mpsc::Receiver<NetworkManagementCommand>,
}

impl NetworkWorker {
    /// Creates a new NetworkWorker
    ///
    /// # Arguments
    /// * cfg: Network configuration.
    /// * listener: Listener part of the establisher.
    /// * establisher: The connection establisher.
    /// * peer_info_db: Database with peer information.
    /// * controller_command_rx: Channel receiving network commands.
    /// * controller_event_tx: Channel sending out network events.
    /// * controller_manager_rx: Channel receiving network management commands.
    pub fn new(
        cfg: NetworkConfig,
        listener: Listener,
        establisher: Establisher,
        peer_info_db: PeerInfoDatabase,
        controller_command_rx: mpsc::Receiver<NetworkCommand>,
        controller_event_tx: mpsc::Sender<NetworkEvent>,
        controller_manager_rx: mpsc::Receiver<NetworkManagementCommand>,
    ) -> NetworkWorker {
        NetworkWorker {
            cfg,
            listener,
            establisher,
            peer_info_db,
            controller_command_rx,
            controller_event_tx,
            controller_manager_rx,
        }
    }

    /// Runs the main loop of the network_worker
    /// There is a tokio::select! insside the loop
    pub async fn run_loop(mut self) -> Result<(), CommunicationError> {
        let mut out_connecting_futures = FuturesUnordered::new();
        let mut cur_connection_id = ConnectionId::default();
        let mut active_connections: HashMap<ConnectionId, (IpAddr, bool)> = HashMap::new(); // ip, is_outgoing

        // wake up the controller at a regular interval to retry connections
        let mut wakeup_interval = tokio::time::interval(self.cfg.wakeup_interval.to_duration());

        loop {
            {
                // try to connect to candidate IPs
                let candidate_ips = self.peer_info_db.get_out_connection_candidate_ips()?;
                for ip in candidate_ips {
                    debug!("starting outgoing connection attempt towards ip={:?}", ip);
                    massa_trace!("out_connection_attempt_start", { "ip": ip });
                    self.peer_info_db.new_out_connection_attempt(&ip)?;
                    let mut connector = self
                        .establisher
                        .get_connector(self.cfg.connect_timeout)
                        .await?;
                    let addr = SocketAddr::new(ip, self.cfg.protocol_port);
                    out_connecting_futures.push(async move {
                        match connector.connect(addr).await {
                            Ok((reader, writer)) => (addr.ip(), Ok((reader, writer))),
                            Err(e) => (addr.ip(), Err(e)),
                        }
                    });
                }
            }

            tokio::select! {
                // listen to manager commands
                cmd = self.controller_manager_rx.recv() => match cmd {
                    None => break,
                    Some(_) => {}
                },

                // wake up interval
                _ = wakeup_interval.tick() => {},

                // peer feedback event
                Some(cmd) = self.controller_command_rx.recv() => manage_network_command(
                    cmd,
                    &mut self.peer_info_db,
                    &mut active_connections,
                    &self.controller_event_tx
                ).await?,

                // out-connector event
                Some((ip_addr, res)) = out_connecting_futures.next() => manage_out_connections(
                    res,
                    ip_addr,
                    &mut self.peer_info_db,
                    &mut cur_connection_id,
                    &mut active_connections,
                    &self.controller_event_tx,
                ).await?,

                // listener socket received
                res = self.listener.accept() => manage_in_connections(
                    res,
                    &mut self.peer_info_db,
                    &mut cur_connection_id,
                    &mut active_connections,
                    &self.controller_event_tx
                ).await?
            }
        }

        // wait for out-connectors to finish
        while let Some(_) = out_connecting_futures.next().await {}

        // stop peer info db
        self.peer_info_db.stop().await?;
        Ok(())
    }
}

/// Manages network commands
/// Only used inside worker's run_loop
///
/// # Arguments
/// * cmd : command to process.
/// * peer_info_db: Database with peer information.
/// * active_connections: hashmap linking connection id to ipAddr to wether connection is outgoing (true)
/// * event_tx: channel to send network events out.
async fn manage_network_command(
    cmd: NetworkCommand,
    peer_info_db: &mut PeerInfoDatabase,
    active_connections: &mut HashMap<ConnectionId, (IpAddr, bool)>,
    event_tx: &mpsc::Sender<NetworkEvent>,
) -> Result<(), CommunicationError> {
    match cmd {
        NetworkCommand::MergeAdvertisedPeerList(ips) => {
            debug!("merging incoming peer list: {:?}", ips);
            massa_trace!("merge_incoming_peer_list", { "ips": ips });
            peer_info_db.merge_candidate_peers(&ips)?;
        }
        NetworkCommand::GetAdvertisablePeerList(response_tx) => {
            response_tx
                .send(peer_info_db.get_advertisable_peer_ips())
                .map_err(|_| {
                    CommunicationError::ChannelError(
                        "could not send GetAdvertisablePeerListChannelError upstream".into(),
                    )
                })?;
        }
        NetworkCommand::ConnectionClosed((id, reason)) => {
            let (ip, is_outgoing) = active_connections
                .remove(&id)
                .ok_or(CommunicationError::ActiveConnectionMissing(id))?;
            debug!(
                "connection closed connedtion_id={:?}, ip={:?}, reason={:?}",
                id, ip, reason
            );
            massa_trace!("connection_closed", {
                "connnection_id": id,
                "ip": ip,
                "reason": reason
            });
            match reason {
                ConnectionClosureReason::Normal => {}
                ConnectionClosureReason::Failed => {
                    peer_info_db.peer_failed(&ip)?;
                }
                ConnectionClosureReason::Banned => {
                    peer_info_db.peer_banned(&ip)?;
                    // notify all other banned connections to close
                    // note: they must close using Normal reason or else risk of feedback loop
                    let target_ids =
                        active_connections
                            .iter()
                            .filter_map(|(other_id, (other_ip, _))| {
                                if *other_ip == ip {
                                    Some(other_id)
                                } else {
                                    None
                                }
                            });
                    for target_id in target_ids {
                        event_tx
                            .send(NetworkEvent::ConnectionBanned(*target_id))
                            .await
                            .map_err(|_| {
                                CommunicationError::ChannelError(
                                    "could not send connection banned notification upstream".into(),
                                )
                            })?;
                    }
                }
            }
            if is_outgoing {
                peer_info_db.out_connection_closed(&ip)?;
            } else {
                peer_info_db.in_connection_closed(&ip)?;
            }
        }
        NetworkCommand::ConnectionAlive(id) => {
            let (ip, _) = active_connections
                .get(&id)
                .ok_or(CommunicationError::ActiveConnectionMissing(id))?;
            peer_info_db.peer_alive(&ip)?;
        }
        NetworkCommand::GetPeers(response_tx) => {
            response_tx
                .send(peer_info_db.get_peers().clone())
                .map_err(|_| {
                    CommunicationError::ChannelError(
                        "could not send GetPeersChannelError upstream".into(),
                    )
                })?;
        }
    }
    Ok(())
}

/// Manages out connection
/// Only used inside worker's run_loop
///
/// # Arguments
/// * res : (reader, writer) in a result comming out of out_connecting_futures
/// * peer_info_db: Database with peer information.
/// * cur_connection_id : connection id of the node we are trying to reach
/// * active_connections: hashmap linking connection id to ipAddr to wether connection is outgoing (true)
/// * event_tx: channel to send network events out.
async fn manage_out_connections(
    res: tokio::io::Result<(ReadHalf, WriteHalf)>,
    ip_addr: IpAddr,
    peer_info_db: &mut PeerInfoDatabase,
    cur_connection_id: &mut ConnectionId,
    active_connections: &mut HashMap<ConnectionId, (IpAddr, bool)>,
    event_tx: &mpsc::Sender<NetworkEvent>,
) -> Result<(), CommunicationError> {
    match res {
        Ok((reader, writer)) => {
            if peer_info_db.try_out_connection_attempt_success(&ip_addr)? {
                // outgoing connection established
                let connection_id = *cur_connection_id;
                debug!(
                    "out connection towards ip={:?} established => connection_id={:?}",
                    ip_addr, connection_id
                );
                massa_trace!("out_connection_established", {
                    "ip": ip_addr,
                    "connection_id": connection_id
                });
                cur_connection_id.0 += 1;
                active_connections.insert(connection_id, (ip_addr, true));
                event_tx
                    .send(NetworkEvent::NewConnection((connection_id, reader, writer)))
                    .await
                    .map_err(|_| {
                        CommunicationError::ChannelError(
                            "could not send new out connection notification".into(),
                        )
                    })?;
            } else {
                debug!("out connection towards ip={:?} refused", ip_addr);
                massa_trace!("out_connection_refused", { "ip": ip_addr });
            }
        }
        Err(err) => {
            debug!(
                "outgoing connection attempt towards ip={:?} failed: {:?}",
                ip_addr, err
            );
            massa_trace!("out_connection_attempt_failed", {
                "ip": ip_addr,
                "err": err.to_string()
            });
            peer_info_db.out_connection_attempt_failed(&ip_addr)?;
        }
    }
    Ok(())
}

/// Manages in connection
/// Only used inside worker's run_loop
///
/// # Arguments
/// * res : (reader, writer, socketAddr) in a result comming out of the listener
/// * peer_info_db: Database with peer information.
/// * cur_connection_id : connection id of the node we are trying to reach
/// * active_connections: hashmap linking connection id to ipAddr to wether connection is outgoing (true)
/// * event_tx: channel to send network events out.
async fn manage_in_connections(
    res: std::io::Result<(ReadHalf, WriteHalf, SocketAddr)>,
    peer_info_db: &mut PeerInfoDatabase,
    cur_connection_id: &mut ConnectionId,
    active_connections: &mut HashMap<ConnectionId, (IpAddr, bool)>,
    event_tx: &mpsc::Sender<NetworkEvent>,
) -> Result<(), CommunicationError> {
    match res {
        Ok((reader, writer, remote_addr)) => {
            if peer_info_db.try_new_in_connection(&remote_addr.ip())? {
                let connection_id = *cur_connection_id;
                debug!(
                    "inbound connection from addr={:?} succeeded => connection_id={:?}",
                    remote_addr, connection_id
                );
                massa_trace!("in_connection_established", {
                    "ip": remote_addr.ip(),
                    "connection_id": connection_id
                });
                cur_connection_id.0 += 1;
                active_connections.insert(connection_id, (remote_addr.ip(), false));
                event_tx
                    .send(NetworkEvent::NewConnection((connection_id, reader, writer)))
                    .await
                    .map_err(|_| {
                        CommunicationError::ChannelError(
                            "could not send new in connection notification".into(),
                        )
                    })?;
            } else {
                debug!("inbound connection from addr={:?} refused", remote_addr);
                massa_trace!("in_connection_refused", {"ip": remote_addr.ip()});
            }
        }
        Err(err) => {
            debug!("connection accept failed: {:?}", err);
            massa_trace!("in_connection_failed", {"err": err.to_string()});
        }
    }
    Ok(())
}
