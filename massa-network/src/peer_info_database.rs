// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::{NetworkConnectionErrorType, NetworkError};
use crate::settings::NetworkSettings;
use itertools::Itertools;
use massa_logging::massa_trace;
use massa_models::constants::MAX_ADVERTISE_LENGTH;
use massa_time::MassaTime;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use std::collections::{hash_map, HashMap};
use std::net::IpAddr;
use std::path::Path;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use tracing::{trace, warn};

/// All information concerning a peer is here
#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
pub struct PeerInfo {
    /// Peer ip address.
    pub ip: IpAddr,
    /// If peer is banned.
    pub banned: bool,
    /// If peer is boostrap, ie peer was in initial peer file
    pub bootstrap: bool,
    /// Time in millis when peer was last alive
    pub last_alive: Option<MassaTime>,
    /// Time in millis of peer's last failure
    pub last_failure: Option<MassaTime>,
    /// Whether peer was promoted through another peer
    pub advertised: bool,

    /// Current number of active out connection attempts with that peer.
    /// Isn't dump into peer file.
    #[serde(default = "usize::default")]
    pub active_out_connection_attempts: usize,
    /// Current number of active out connections with that peer.
    /// Isn't dump into peer file.
    #[serde(default = "usize::default")]
    pub active_out_connections: usize,
    /// Current number of active in connections with that peer.
    /// Isn't dump into peer file.
    #[serde(default = "usize::default")]
    pub active_in_connections: usize,
}

impl PeerInfo {
    /// Returns true if there is at least one connection attempt /
    ///  one active connection in either direction
    /// with this peer
    pub(crate) fn is_active(&self) -> bool {
        self.active_out_connection_attempts > 0
            || self.active_out_connections > 0
            || self.active_in_connections > 0
    }
}

/// Contains all information about every peers we know about.
pub struct PeerInfoDatabase {
    /// Network configuration.
    pub(crate) network_settings: NetworkSettings,
    /// Maps an ip address to peer's info
    pub peers: HashMap<IpAddr, PeerInfo>,
    /// Handle on the task managing the dump
    pub(crate) saver_join_handle: JoinHandle<()>,
    /// Monitor changed peers.
    pub(crate) saver_watch_tx: watch::Sender<HashMap<IpAddr, PeerInfo>>,
    /// Total number of active out bootstrap connection attempts.
    pub(crate) active_out_bootstrap_connection_attempts: usize,
    /// Total number of active out non-bootstrap connection attempts.
    pub(crate) active_out_nonbootstrap_connection_attempts: usize,
    /// Total number of active bootstrap connections.
    pub(crate) active_bootstrap_connections: usize, // TODO: in or out connections?
    /// Total number of active out non-bootstrap connections.
    pub active_out_nonbootstrap_connections: usize,
    /// Total number of active in non-bootstrap connections
    pub active_in_nonbootstrap_connections: usize,
    /// Every wakeup_interval we try to establish a connection with known inactive peers
    pub(crate) wakeup_interval: MassaTime,
    /// Clock compensation.
    pub(crate) clock_compensation: i64,
}

/// Saves banned, advertised and bootstrap peers to a file.
///
/// # Arguments
/// * peers: peers to save
/// * file_path : path to the file
async fn dump_peers(
    peers: &HashMap<IpAddr, PeerInfo>,
    file_path: &Path,
) -> Result<(), NetworkError> {
    let peer_vec: Vec<Value> = peers
        .values()
        .filter(|v| v.banned || v.advertised || v.bootstrap)
        //        .cloned()
        .map(|peer| {
            json!({
                "ip": peer.ip,
                "banned": peer.banned,
                "bootstrap": peer.bootstrap,
                "last_alive": peer.last_alive,
                "last_failure": peer.last_failure,
                "advertised": peer.advertised,
            })
        })
        .collect();

    tokio::fs::write(file_path, serde_json::to_string_pretty(&peer_vec)?).await?;

    Ok(())
}

/// Cleans up the peer database using max values
/// provided by NetworkConfig.ProtocolConfig.
/// If opt_new_peers is provided, adds its contents as well.
///
/// Note: only non-active, non-bootstrap peers are counted when clipping to size limits.
///
/// Arguments : cfg : NetworkConfig
/// peers : peers to clean up
/// opt_new_peers : optional peers to add to the database
pub(crate) fn cleanup_peers(
    cfg: &NetworkSettings,
    peers: &mut HashMap<IpAddr, PeerInfo>,
    opt_new_peers: Option<&Vec<IpAddr>>,
    clock_compensation: i64,
    ban_timeout: MassaTime,
) -> Result<(), NetworkError> {
    // filter and map new peers, remove duplicates
    let mut res_new_peers: Vec<PeerInfo> = if let Some(new_peers) = opt_new_peers {
        new_peers
            .iter()
            .unique()
            .filter(|&ip| {
                if let Some(mut p) = peers.get_mut(ip) {
                    // avoid already-known IPs, but mark them as advertised
                    p.advertised = true;
                    return false;
                }
                if !ip.is_global() {
                    // avoid non-global IPs
                    return false;
                }
                if let Some(our_ip) = cfg.routable_ip {
                    // avoid our own IP
                    if ip == &our_ip {
                        return false;
                    }
                }
                true
            })
            .take(MAX_ADVERTISE_LENGTH as usize)
            .map(|&ip| PeerInfo {
                ip,
                banned: false,
                bootstrap: false,
                last_alive: None,
                last_failure: None,
                advertised: true,
                active_out_connection_attempts: 0,
                active_out_connections: 0,
                active_in_connections: 0,
            })
            .collect()
    } else {
        Vec::new()
    };

    // split between peers that need to be kept (keep_peers),
    // inactive banned peers (banned_peers)
    // and other inactive but advertised peers (idle_peers)
    // drop other peers (inactive non-advertised, non-keep)
    let mut keep_peers: Vec<PeerInfo> = Vec::new();
    let mut banned_peers: Vec<PeerInfo> = Vec::new();
    let mut idle_peers: Vec<PeerInfo> = Vec::new();
    for (ip, p) in peers.drain() {
        if !ip.is_global() {
            // avoid non-global IPs
            continue;
        }
        if let Some(our_ip) = cfg.routable_ip {
            // avoid our own IP
            if ip == our_ip {
                continue;
            }
        }
        if p.bootstrap || p.is_active() {
            keep_peers.push(p);
        } else if p.banned {
            banned_peers.push(p);
        } else if p.advertised {
            idle_peers.push(p);
        } // else drop peer (idle and not advertised)
    }

    // append new peers to idle_peers
    // stable sort to keep new_peers order,
    // also prefer existing peers over new ones
    // truncate to max length
    idle_peers.append(&mut res_new_peers);
    idle_peers.sort_by_key(|&p| (std::cmp::Reverse(p.last_alive), p.last_failure));
    idle_peers.truncate(cfg.max_idle_peers);

    // sort and truncate inactive banned peers
    // forget about old banned peers
    let ban_limit = MassaTime::compensated_now(clock_compensation)?.saturating_sub(ban_timeout);
    banned_peers.retain(|p| p.last_failure.map_or(false, |v| v >= ban_limit));
    banned_peers.sort_unstable_by_key(|&p| (std::cmp::Reverse(p.last_failure), p.last_alive));
    banned_peers.truncate(cfg.max_banned_peers);

    // gather everything back
    peers.extend(keep_peers.into_iter().map(|p| (p.ip, p)));
    peers.extend(banned_peers.into_iter().map(|p| (p.ip, p)));
    peers.extend(idle_peers.into_iter().map(|p| (p.ip, p)));
    Ok(())
}

impl PeerInfoDatabase {
    /// Creates new peerInfoDatabase from NetworkConfig.
    /// will only emit a warning if peers dumping failed.
    ///
    /// # Argument
    /// * cfg : network configuration
    pub async fn new(cfg: &NetworkSettings, clock_compensation: i64) -> Result<Self, NetworkError> {
        // wakeup interval
        let wakeup_interval = cfg.wakeup_interval;

        // load from initial file
        let mut peers = serde_json::from_str::<Vec<PeerInfo>>(
            &tokio::fs::read_to_string(&cfg.initial_peers_file).await?,
        )?
        .into_iter()
        .map(|p| (p.ip, p))
        .collect::<HashMap<IpAddr, PeerInfo>>();
        if cfg.peers_file.is_file() {
            peers.extend(
                // previously known peers
                serde_json::from_str::<Vec<PeerInfo>>(
                    &tokio::fs::read_to_string(&cfg.peers_file).await?,
                )?
                .into_iter()
                .map(|p| (p.ip, p)),
            );
        }

        // cleanup
        cleanup_peers(cfg, &mut peers, None, clock_compensation, cfg.ban_timeout)?;

        // setup saver
        let peers_file = cfg.peers_file.clone();
        let peers_file_dump_interval = cfg.peers_file_dump_interval;
        let (saver_watch_tx, mut saver_watch_rx) = watch::channel(peers.clone());
        let mut need_dump = false;
        let saver_join_handle = tokio::spawn(async move {
            let delay = sleep(Duration::from_millis(0));
            tokio::pin!(delay);
            loop {
                tokio::select! {
                    opt_p = saver_watch_rx.changed() => match opt_p {
                        Ok(_) => if !need_dump {
                            delay.set(sleep(peers_file_dump_interval.to_duration()));
                            need_dump = true;
                        },
                        Err(_) => break
                    },
                    _ = &mut delay, if need_dump => {
                        let to_dump = saver_watch_rx.borrow().clone();
                        match dump_peers(&to_dump, &peers_file).await {
                            Ok(_) => { need_dump = false; },
                            Err(e) => {
                                warn!("could not dump peers to file: {}", e);
                                delay.set(sleep(peers_file_dump_interval.to_duration()));
                            }
                        }
                    }
                }
            }
        });

        // return struct
        Ok(PeerInfoDatabase {
            network_settings: cfg.clone(),
            peers,
            saver_join_handle,
            saver_watch_tx,
            active_out_bootstrap_connection_attempts: 0,
            active_bootstrap_connections: 0,
            active_out_nonbootstrap_connection_attempts: 0,
            active_out_nonbootstrap_connections: 0,
            active_in_nonbootstrap_connections: 0,
            wakeup_interval,
            clock_compensation,
        })
    }

    /// Refreshes the peer list. Should be called at regular intervals.
    /// Performs multiple cleanup tasks e.g. remove old banned peers
    pub fn update(&mut self) -> Result<(), NetworkError> {
        cleanup_peers(
            &self.network_settings,
            &mut self.peers,
            None,
            self.clock_compensation,
            self.network_settings.ban_timeout,
        )?;
        Ok(())
    }

    /// Request peers dump to file
    fn request_dump(&self) -> Result<(), NetworkError> {
        trace!("before sending self.peers.clone() from saver_watch_tx in peer_info_database request_dump");
        let res = self
            .saver_watch_tx
            .send(self.peers.clone())
            .map_err(|_| NetworkError::ChannelError("could not send on saver_watch_tx".into()));
        trace!("before sending self.peers.clone() from saver_watch_tx in peer_info_database request_dump");
        res
    }

    pub async fn unban(&mut self, ips: Vec<IpAddr>) -> Result<(), NetworkError> {
        for ip in ips.into_iter() {
            if let Some(peer) = self.peers.get_mut(&ip) {
                peer.banned = false;
            } else {
                return Ok(());
            }
        }
        cleanup_peers(
            &self.network_settings,
            &mut self.peers,
            None,
            self.clock_compensation,
            self.network_settings.ban_timeout,
        )?;
        Ok(())
    }

    /// Cleanly closes peerInfoDatabase, performing one last peer dump.
    /// A warning is raised on dump failure.
    pub async fn stop(self) -> Result<(), NetworkError> {
        drop(self.saver_watch_tx);
        self.saver_join_handle.await?;
        if let Err(e) = dump_peers(&self.peers, &self.network_settings.peers_file).await {
            warn!("could not dump peers to file: {}", e);
        }
        Ok(())
    }

    /// Gets available out connection attempts
    /// according to NetworkConfig and current connections and connection attempts.
    // returns (count for bootstrap, count for non-bootstrap)
    pub fn get_available_out_connection_attempts(&self) -> (usize, usize) {
        let bootstrap_count = std::cmp::min(
            self.network_settings
                .target_bootstrap_connections
                .saturating_sub(self.active_out_bootstrap_connection_attempts)
                .saturating_sub(self.active_bootstrap_connections),
            self.network_settings
                .max_out_bootstrap_connection_attempts
                .saturating_sub(self.active_out_bootstrap_connection_attempts),
        );

        let nonbootstrap_count = std::cmp::min(
            self.network_settings
                .target_out_nonbootstrap_connections
                .saturating_sub(self.active_out_nonbootstrap_connection_attempts)
                .saturating_sub(self.active_out_nonbootstrap_connections),
            self.network_settings
                .max_out_nonbootstrap_connection_attempts
                .saturating_sub(self.active_out_nonbootstrap_connection_attempts),
        );

        (bootstrap_count, nonbootstrap_count)
    }

    /// Sorts peers by ( last_failure, rev(last_success) )
    /// and returns as many peers as there are available slots to attempt outgoing connections to.
    pub fn get_out_connection_candidate_ips(&self) -> Result<Vec<IpAddr>, NetworkError> {
        /*
            get_connect_candidate_ips must return the full sorted list where:
                advertised && !banned && out_connection_attempts==0 && out_connections==0 && in_connections=0
                sorted_by = ( last_failure, rev(last_success) )
        */
        let (available_slots_bootstrap, available_slots_nonbootstrap) =
            self.get_available_out_connection_attempts();
        let mut res_ips: Vec<IpAddr> = Vec::new();

        if available_slots_bootstrap > 0 {
            let now = MassaTime::compensated_now(self.clock_compensation)?;
            let mut sorted_peers: Vec<PeerInfo> = self
                .peers
                .values()
                .filter(|&p| {
                    if !p.bootstrap || !p.advertised || p.banned || p.is_active() {
                        return false;
                    }
                    if let Some(last_failure) = p.last_failure {
                        if let Some(last_alive) = p.last_alive {
                            if last_alive > last_failure {
                                return true;
                            }
                        }
                        return now
                            .saturating_sub(last_failure)
                            .saturating_sub(self.wakeup_interval)
                            > MassaTime::from(0u64);
                    }
                    true
                })
                .copied()
                .collect();
            sorted_peers
                .sort_unstable_by_key(|&p| (p.last_failure, std::cmp::Reverse(p.last_alive)));
            res_ips.extend(
                sorted_peers
                    .into_iter()
                    .take(available_slots_bootstrap)
                    .map(|p| p.ip),
            )
        }

        if available_slots_nonbootstrap > 0 {
            let now = MassaTime::compensated_now(self.clock_compensation)?;
            let mut sorted_peers: Vec<PeerInfo> = self
                .peers
                .values()
                .filter(|&p| {
                    if !(!p.bootstrap && p.advertised && !p.banned && !p.is_active()) {
                        return false;
                    }
                    if let Some(last_failure) = p.last_failure {
                        if let Some(last_alive) = p.last_alive {
                            if last_alive > last_failure {
                                return true;
                            }
                        }
                        return now
                            .saturating_sub(last_failure)
                            .saturating_sub(self.wakeup_interval)
                            > MassaTime::from(0u64);
                    }
                    true
                })
                .copied()
                .collect();
            sorted_peers
                .sort_unstable_by_key(|&p| (p.last_failure, std::cmp::Reverse(p.last_alive)));
            res_ips.extend(
                sorted_peers
                    .into_iter()
                    .take(available_slots_nonbootstrap)
                    .map(|p| p.ip),
            )
        }

        Ok(res_ips)
    }

    /// returns Hashmap of ipAddrs -> Peerinfo
    pub fn get_peers(&self) -> &HashMap<IpAddr, PeerInfo> {
        &self.peers
    }

    /// Returns a vec of advertisable IpAddrs sorted by ( last_failure, rev(last_success) )
    pub fn get_advertisable_peer_ips(&self) -> Vec<IpAddr> {
        let mut sorted_peers: Vec<PeerInfo> = self
            .peers
            .values()
            .filter(|&p| (p.advertised && !p.banned))
            .copied()
            .collect();
        sorted_peers.sort_unstable_by_key(|&p| (std::cmp::Reverse(p.last_alive), p.last_failure));
        let mut sorted_ips: Vec<IpAddr> = sorted_peers
            .into_iter()
            .take(MAX_ADVERTISE_LENGTH as usize)
            .map(|p| p.ip)
            .collect();
        if let Some(our_ip) = self.network_settings.routable_ip {
            sorted_ips.insert(0, our_ip);
            sorted_ips.truncate(MAX_ADVERTISE_LENGTH as usize);
        }
        sorted_ips
    }

    /// Acknowledges a new out connection attempt to ip.
    ///
    /// # Argument
    /// ip: ipAddr we are now connected to
    pub fn new_out_connection_attempt(&mut self, ip: &IpAddr) -> Result<(), NetworkError> {
        if !ip.is_global() {
            return Err(NetworkError::InvalidIpError(*ip));
        }
        let (available_bootstrap_conns, available_nonbootstrap_conns) =
            self.get_available_out_connection_attempts();
        let peer = self.peers.get_mut(ip).ok_or({
            NetworkError::PeerConnectionError(NetworkConnectionErrorType::PeerInfoNotFoundError(
                *ip,
            ))
        })?;
        if peer.bootstrap {
            if available_bootstrap_conns == 0 {
                return Err(NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::ToManyConnectionAttempt(*ip),
                ));
            }
        } else if available_nonbootstrap_conns == 0 {
            return Err(NetworkError::PeerConnectionError(
                NetworkConnectionErrorType::ToManyConnectionAttempt(*ip),
            ));
        }
        peer.active_out_connection_attempts += 1;
        if peer.bootstrap {
            self.active_out_bootstrap_connection_attempts += 1;
        } else {
            self.active_out_nonbootstrap_connection_attempts += 1;
        }
        Ok(())
    }

    /// Merges new_peers with our peers using the cleanup_peers function.
    /// A dump is requested afterwards.
    ///
    /// # Argument
    /// new_peers: peers we are trying to merge
    pub fn merge_candidate_peers(&mut self, new_peers: &[IpAddr]) -> Result<(), NetworkError> {
        if new_peers.is_empty() {
            return Ok(());
        }
        cleanup_peers(
            &self.network_settings,
            &mut self.peers,
            Some(&new_peers.to_vec()),
            self.clock_compensation,
            self.network_settings.ban_timeout,
        )?;
        self.request_dump()
    }

    /// Sets the peer status as alive.
    /// Requests a subsequent dump.
    ///
    /// # Argument
    /// * ip : ip address of the considered peer.
    pub fn peer_alive(&mut self, ip: &IpAddr) -> Result<(), NetworkError> {
        self.peers
            .get_mut(ip)
            .ok_or({
                NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::PeerInfoNotFoundError(*ip),
                )
            })?
            .last_alive = Some(MassaTime::compensated_now(self.clock_compensation)?);
        self.request_dump()
    }

    /// Sets the peer status as failed.
    /// Requests a dump.
    ///
    /// # Argument
    /// * ip : ip address of the considered peer.
    pub fn peer_failed(&mut self, ip: &IpAddr) -> Result<(), NetworkError> {
        self.peers
            .get_mut(ip)
            .ok_or({
                NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::PeerInfoNotFoundError(*ip),
                )
            })?
            .last_failure = Some(MassaTime::compensated_now(self.clock_compensation)?);
        self.request_dump()
    }

    /// Sets that the peer is banned now.
    /// If the peer is not active, the database is cleaned up.
    /// A dump is requested.
    ///
    /// # Argument
    /// * ip : ip address of the considered peer.
    pub fn peer_banned(&mut self, ip: &IpAddr) -> Result<(), NetworkError> {
        let peer = self.peers.entry(*ip).or_insert_with(|| PeerInfo {
            ip: *ip,
            banned: false,
            bootstrap: false,
            last_alive: None,
            last_failure: None,
            advertised: false,
            active_out_connection_attempts: 0,
            active_out_connections: 0,
            active_in_connections: 0,
        });
        peer.last_failure = Some(MassaTime::compensated_now(self.clock_compensation)?);
        if !peer.banned {
            peer.banned = true;
            if !peer.is_active() && !peer.bootstrap {
                cleanup_peers(
                    &self.network_settings,
                    &mut self.peers,
                    None,
                    self.clock_compensation,
                    self.network_settings.ban_timeout,
                )?;
            }
        }
        self.request_dump()
    }

    /// Notifies of a closed outgoing connection.
    ///
    /// If the peer is not active nor bootstrap,
    /// peers are cleaned up and a dump is requested
    ///
    /// # Argument
    /// * ip : ip address of the considered peer.
    pub fn out_connection_closed(&mut self, ip: &IpAddr) -> Result<(), NetworkError> {
        let peer = self.peers.get_mut(ip).ok_or({
            NetworkError::PeerConnectionError(NetworkConnectionErrorType::PeerInfoNotFoundError(
                *ip,
            ))
        })?;
        if (peer.bootstrap && self.active_bootstrap_connections == 0)
            || (!peer.bootstrap && self.active_out_nonbootstrap_connections == 0)
            || peer.active_out_connections == 0
        {
            return Err(NetworkError::PeerConnectionError(
                NetworkConnectionErrorType::CloseConnectionWithNoConnectionToClose(*ip),
            ));
        }
        peer.active_out_connections -= 1;
        if peer.bootstrap {
            self.active_bootstrap_connections -= 1;
        } else {
            self.active_out_nonbootstrap_connections -= 1;
        }

        if !peer.is_active() && !peer.bootstrap {
            cleanup_peers(
                &self.network_settings,
                &mut self.peers,
                None,
                self.clock_compensation,
                self.network_settings.ban_timeout,
            )?;
            self.request_dump()
        } else {
            Ok(())
        }
    }

    /// Notifies that an inbound connection is closed.
    ///
    /// If the peer is not active nor bootstrap
    /// peers are cleaned up and a dump is requested.
    ///
    /// # Argument
    /// * ip : ip address of the considered peer.
    pub fn in_connection_closed(&mut self, ip: &IpAddr) -> Result<(), NetworkError> {
        let peer = self.peers.get_mut(ip).ok_or({
            NetworkError::PeerConnectionError(NetworkConnectionErrorType::PeerInfoNotFoundError(
                *ip,
            ))
        })?;

        if (peer.bootstrap && self.active_bootstrap_connections == 0)
            || (!peer.bootstrap && self.active_in_nonbootstrap_connections == 0)
            || peer.active_in_connections == 0
        {
            return Err(NetworkError::PeerConnectionError(
                NetworkConnectionErrorType::CloseConnectionWithNoConnectionToClose(*ip),
            ));
        }
        peer.active_in_connections -= 1;
        if peer.bootstrap {
            self.active_bootstrap_connections -= 1;
        } else {
            self.active_in_nonbootstrap_connections -= 1;
        }
        if !peer.is_active() && !peer.bootstrap {
            cleanup_peers(
                &self.network_settings,
                &mut self.peers,
                None,
                self.clock_compensation,
                self.network_settings.ban_timeout,
            )?;
            self.request_dump()
        } else {
            Ok(())
        }
    }

    /// Yay an out connection attempt succeeded.
    /// returns false if there are no slots left for out connections.
    /// The peer is set to advertised.
    ///
    /// A dump is requested.
    ///
    /// # Argument
    /// * ip : ip address of the considered peer.
    pub fn try_out_connection_attempt_success(
        &mut self,
        ip: &IpAddr,
    ) -> Result<bool, NetworkError> {
        // a connection attempt succeeded
        // remove out connection attempt and add out connection
        let peer = self.peers.get_mut(ip).ok_or({
            NetworkError::PeerConnectionError(NetworkConnectionErrorType::PeerInfoNotFoundError(
                *ip,
            ))
        })?;

        if (peer.bootstrap && self.active_out_bootstrap_connection_attempts == 0)
            || (!peer.bootstrap && self.active_out_nonbootstrap_connection_attempts == 0)
            || (peer.active_out_connection_attempts == 0)
        {
            return Err(NetworkError::PeerConnectionError(
                NetworkConnectionErrorType::ToManyConnectionAttempt(*ip),
            ));
        }
        if (peer.bootstrap
            && self.active_bootstrap_connections
                >= self.network_settings.target_bootstrap_connections)
            || (!peer.bootstrap
                && self.active_out_nonbootstrap_connections
                    >= self.network_settings.target_out_nonbootstrap_connections)
        {
            return Ok(false);
        }
        peer.active_out_connection_attempts -= 1;
        if peer.bootstrap {
            self.active_out_bootstrap_connection_attempts -= 1;
        } else {
            self.active_out_nonbootstrap_connection_attempts -= 1;
        }
        peer.advertised = true; // we just connected to it. Assume advertised.
        if peer.banned {
            peer.last_failure = Some(MassaTime::compensated_now(self.clock_compensation)?);
            if !peer.is_active() && !peer.bootstrap {
                cleanup_peers(
                    &self.network_settings,
                    &mut self.peers,
                    None,
                    self.clock_compensation,
                    self.network_settings.ban_timeout,
                )?;
            }
            self.request_dump()?;
            return Ok(false);
        }
        peer.active_out_connections += 1;
        if peer.bootstrap {
            self.active_bootstrap_connections += 1;
        } else {
            self.active_out_nonbootstrap_connections += 1;
        }
        self.request_dump()?;
        Ok(true)
    }

    /// Oh no an out connection attempt failed.
    ///
    /// A dump is requested.
    ///
    /// # Argument
    /// * ip : ip address of the considered peer.
    pub fn out_connection_attempt_failed(&mut self, ip: &IpAddr) -> Result<(), NetworkError> {
        let peer = self.peers.get_mut(ip).ok_or({
            NetworkError::PeerConnectionError(NetworkConnectionErrorType::PeerInfoNotFoundError(
                *ip,
            ))
        })?;
        if (peer.bootstrap && self.active_out_bootstrap_connection_attempts == 0)
            || (!peer.bootstrap && self.active_out_nonbootstrap_connection_attempts == 0)
            || peer.active_out_connection_attempts == 0
        {
            return Err(NetworkError::PeerConnectionError(
                NetworkConnectionErrorType::ToManyConnectionFailure(*ip),
            ));
        }
        peer.active_out_connection_attempts -= 1;
        if peer.bootstrap {
            self.active_out_bootstrap_connection_attempts -= 1;
        } else {
            self.active_out_nonbootstrap_connection_attempts -= 1;
        }
        peer.last_failure = Some(MassaTime::compensated_now(self.clock_compensation)?);
        if !peer.is_active() && !peer.bootstrap {
            cleanup_peers(
                &self.network_settings,
                &mut self.peers,
                None,
                self.clock_compensation,
                self.network_settings.ban_timeout,
            )?;
        }
        self.request_dump()
    }

    /// An ip has successfully connected to us.
    /// returns true if some in slots for connections are left.
    /// If the corresponding peer exists, it is updated,
    /// otherwise it is created (not advertised).
    /// A dump is requested.
    ///
    /// # Argument
    /// * ip : ip address of the considered peer.
    pub fn try_new_in_connection(&mut self, ip: &IpAddr) -> Result<(), NetworkError> {
        // try to create a new input connection, return false if no slots
        if !ip.is_global() || self.network_settings.max_in_connections_per_ip == 0 {
            return Err(NetworkError::PeerConnectionError(
                NetworkConnectionErrorType::MaxPeersConnectionReached(*ip),
            ));
        }
        if let Some(our_ip) = self.network_settings.routable_ip {
            // avoid our own IP
            if *ip == our_ip {
                warn!("incoming connection from our own IP");
                return Err(NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::SelfConnection,
                ));
            }
        }

        let res = match self.peers.entry(*ip) {
            hash_map::Entry::Occupied(mut occ) => {
                let peer = occ.get_mut();
                if (peer.bootstrap
                    && self.active_bootstrap_connections
                        >= self.network_settings.target_bootstrap_connections)
                    || (!peer.bootstrap
                        && self.active_in_nonbootstrap_connections
                            >= self.network_settings.max_in_nonbootstrap_connections)
                {
                    Err(NetworkConnectionErrorType::MaxPeersConnectionReached(*ip))
                } else if peer.banned {
                    massa_trace!("in_connection_refused_peer_banned", {"ip": peer.ip});
                    peer.last_failure = Some(MassaTime::compensated_now(self.clock_compensation)?);
                    self.request_dump()?;
                    Err(NetworkConnectionErrorType::BannedPeerTryingToConnect(*ip))
                } else if peer.active_in_connections
                    >= self.network_settings.max_in_connections_per_ip
                {
                    self.request_dump()?;
                    Err(NetworkConnectionErrorType::MaxPeersConnectionReached(*ip))
                } else {
                    peer.active_in_connections += 1;
                    if peer.bootstrap {
                        self.active_bootstrap_connections += 1;
                    } else {
                        self.active_in_nonbootstrap_connections += 1;
                    }
                    Ok(())
                }
            }
            hash_map::Entry::Vacant(vac) => {
                let mut peer = PeerInfo {
                    ip: *ip,
                    banned: false,
                    bootstrap: false,
                    last_alive: None,
                    last_failure: None,
                    advertised: false,
                    active_out_connection_attempts: 0,
                    active_out_connections: 0,
                    active_in_connections: 0,
                };
                if self.active_in_nonbootstrap_connections
                    >= self.network_settings.max_in_nonbootstrap_connections
                {
                    Err(NetworkConnectionErrorType::MaxPeersConnectionReached(*ip))
                } else if peer.active_in_connections
                    >= self.network_settings.max_in_connections_per_ip
                {
                    self.request_dump()?;
                    Err(NetworkConnectionErrorType::MaxPeersConnectionReached(*ip))
                } else {
                    peer.active_in_connections += 1;
                    vac.insert(peer);
                    self.active_in_nonbootstrap_connections += 1;
                    Ok(())
                }
            }
        };
        if let Err(res) = res {
            return Err(NetworkError::PeerConnectionError(res));
        }
        self.request_dump()?;
        Ok(())
    }
}
