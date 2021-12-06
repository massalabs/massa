// Copyright (c) 2021 MASSA LABS <info@massa.net>

use super::config::NetworkConfig;
use crate::error::{NetworkConnectionErrorType, NetworkError};
use itertools::Itertools;
use logging::massa_trace;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use std::collections::{hash_map, HashMap};
use std::net::IpAddr;
use std::path::Path;
use time::UTime;
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
    pub last_alive: Option<UTime>,
    /// Time in millis of peer's last failure
    pub last_failure: Option<UTime>,
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
    fn is_active(&self) -> bool {
        self.active_out_connection_attempts > 0
            || self.active_out_connections > 0
            || self.active_in_connections > 0
    }
}

/// Contains all information about every peers we know about.
pub struct PeerInfoDatabase {
    /// Network configuration.
    cfg: NetworkConfig,
    /// Maps an ip address to peer's info
    pub peers: HashMap<IpAddr, PeerInfo>,
    /// Handle on the task managing the dump
    saver_join_handle: JoinHandle<()>,
    /// Monitor changed peers.
    saver_watch_tx: watch::Sender<HashMap<IpAddr, PeerInfo>>,
    /// Total number of active out bootstrap connection attempts.
    active_out_bootstrap_connection_attempts: usize,
    /// Total number of active out non-bootstrap connection attempts.
    active_out_nonbootstrap_connection_attempts: usize,
    /// Total number of active bootstrap connections.
    active_bootstrap_connections: usize, // TODO: in or out connections?
    /// Total number of active out non-bootstrap connections.
    pub active_out_nonbootstrap_connections: usize,
    /// Total number of active in non-bootstrap connections
    pub active_in_nonbootstrap_connections: usize,
    /// Every wakeup_interval we try to establish a connection with known inactive peers
    wakeup_interval: UTime,
    /// Clock compensation.
    clock_compensation: i64,
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
fn cleanup_peers(
    cfg: &NetworkConfig,
    peers: &mut HashMap<IpAddr, PeerInfo>,
    opt_new_peers: Option<&Vec<IpAddr>>,
    clock_compensation: i64,
    ban_timeout: UTime,
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
            .take(cfg.max_advertise_length as usize)
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
    let ban_limit = UTime::now(clock_compensation)?.saturating_sub(ban_timeout);
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
    pub async fn new(cfg: &NetworkConfig, clock_compensation: i64) -> Result<Self, NetworkError> {
        // wakeup interval
        let wakeup_interval = cfg.wakeup_interval;

        // load from file
        let mut peers = serde_json::from_str::<Vec<PeerInfo>>(
            &tokio::fs::read_to_string(&cfg.peers_file).await?,
        )?
        .into_iter()
        .map(|p| (p.ip, p))
        .collect::<HashMap<IpAddr, PeerInfo>>();

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
            cfg: cfg.clone(),
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
            &self.cfg,
            &mut self.peers,
            None,
            self.clock_compensation,
            self.cfg.ban_timeout,
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
            &self.cfg,
            &mut self.peers,
            None,
            self.clock_compensation,
            self.cfg.ban_timeout,
        )?;
        Ok(())
    }

    /// Cleanly closes peerInfoDatabase, performing one last peer dump.
    /// A warning is raised on dump failure.
    pub async fn stop(self) -> Result<(), NetworkError> {
        drop(self.saver_watch_tx);
        self.saver_join_handle.await?;
        if let Err(e) = dump_peers(&self.peers, &self.cfg.peers_file).await {
            warn!("could not dump peers to file: {}", e);
        }
        Ok(())
    }

    /// Gets available out connection attempts
    /// according to NetworkConfig and current connections and connection attempts.
    // returns (count for bootstrap, count for non-bootstrap)
    pub fn get_available_out_connection_attempts(&self) -> (usize, usize) {
        let bootstrap_count = std::cmp::min(
            self.cfg
                .target_bootstrap_connections
                .saturating_sub(self.active_out_bootstrap_connection_attempts)
                .saturating_sub(self.active_bootstrap_connections),
            self.cfg
                .max_out_bootstrap_connection_attempts
                .saturating_sub(self.active_out_bootstrap_connection_attempts),
        );

        let nonbootstrap_count = std::cmp::min(
            self.cfg
                .target_out_nonbootstrap_connections
                .saturating_sub(self.active_out_nonbootstrap_connection_attempts)
                .saturating_sub(self.active_out_nonbootstrap_connections),
            self.cfg
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
            let now = UTime::now(self.clock_compensation)?;
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
                            > UTime::from(0u64);
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
            let now = UTime::now(self.clock_compensation)?;
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
                            > UTime::from(0u64);
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
            .take(self.cfg.max_advertise_length as usize)
            .map(|p| p.ip)
            .collect();
        if let Some(our_ip) = self.cfg.routable_ip {
            sorted_ips.insert(0, our_ip);
            sorted_ips.truncate(self.cfg.max_advertise_length as usize);
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
        let peer = self.peers.get_mut(ip).ok_or_else(|| {
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
    pub fn merge_candidate_peers(&mut self, new_peers: &Vec<IpAddr>) -> Result<(), NetworkError> {
        if new_peers.is_empty() {
            return Ok(());
        }
        cleanup_peers(
            &self.cfg,
            &mut self.peers,
            Some(new_peers),
            self.clock_compensation,
            self.cfg.ban_timeout,
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
            .ok_or_else(|| {
                NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::PeerInfoNotFoundError(*ip),
                )
            })?
            .last_alive = Some(UTime::now(self.clock_compensation)?);
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
            .ok_or_else(|| {
                NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::PeerInfoNotFoundError(*ip),
                )
            })?
            .last_failure = Some(UTime::now(self.clock_compensation)?);
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
        peer.last_failure = Some(UTime::now(self.clock_compensation)?);
        if !peer.banned {
            peer.banned = true;
            if !peer.is_active() && !peer.bootstrap {
                cleanup_peers(
                    &self.cfg,
                    &mut self.peers,
                    None,
                    self.clock_compensation,
                    self.cfg.ban_timeout,
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
        let peer = self.peers.get_mut(ip).ok_or_else(|| {
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
                &self.cfg,
                &mut self.peers,
                None,
                self.clock_compensation,
                self.cfg.ban_timeout,
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
        let peer = self.peers.get_mut(ip).ok_or_else(|| {
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
                &self.cfg,
                &mut self.peers,
                None,
                self.clock_compensation,
                self.cfg.ban_timeout,
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
        let peer = self.peers.get_mut(ip).ok_or_else(|| {
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
            && self.active_bootstrap_connections >= self.cfg.target_bootstrap_connections)
            || (!peer.bootstrap
                && self.active_out_nonbootstrap_connections
                    >= self.cfg.target_out_nonbootstrap_connections)
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
            peer.last_failure = Some(UTime::now(self.clock_compensation)?);
            if !peer.is_active() && !peer.bootstrap {
                cleanup_peers(
                    &self.cfg,
                    &mut self.peers,
                    None,
                    self.clock_compensation,
                    self.cfg.ban_timeout,
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
        let peer = self.peers.get_mut(ip).ok_or_else(|| {
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
        peer.last_failure = Some(UTime::now(self.clock_compensation)?);
        if !peer.is_active() && !peer.bootstrap {
            cleanup_peers(
                &self.cfg,
                &mut self.peers,
                None,
                self.clock_compensation,
                self.cfg.ban_timeout,
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
    pub fn try_new_in_connection(&mut self, ip: &IpAddr) -> Result<bool, NetworkError> {
        // try to create a new input connection, return false if no slots
        if !ip.is_global() || self.cfg.max_in_connections_per_ip == 0 {
            return Ok(false);
        }
        if let Some(our_ip) = self.cfg.routable_ip {
            // avoid our own IP
            if *ip == our_ip {
                warn!("incoming connection from our own IP");
                return Ok(false);
            }
        }

        match self.peers.entry(*ip) {
            hash_map::Entry::Occupied(mut occ) => {
                let peer = occ.get_mut();
                if (peer.bootstrap
                    && self.active_bootstrap_connections >= self.cfg.target_bootstrap_connections)
                    || (!peer.bootstrap
                        && self.active_in_nonbootstrap_connections
                            >= self.cfg.max_in_nonbootstrap_connections)
                {
                    return Ok(false);
                }
                if peer.banned {
                    massa_trace!("in_connection_refused_peer_banned", {"ip": peer.ip});
                    peer.last_failure = Some(UTime::now(self.clock_compensation)?);
                    self.request_dump()?;
                    return Ok(false);
                }
                if peer.active_in_connections >= self.cfg.max_in_connections_per_ip {
                    self.request_dump()?;
                    return Ok(false);
                }
                peer.active_in_connections += 1;
                if peer.bootstrap {
                    self.active_bootstrap_connections += 1;
                } else {
                    self.active_in_nonbootstrap_connections += 1;
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
                    >= self.cfg.max_in_nonbootstrap_connections
                {
                    return Ok(false);
                }
                if peer.active_in_connections >= self.cfg.max_in_connections_per_ip {
                    self.request_dump()?;
                    return Ok(false);
                }
                peer.active_in_connections += 1;
                vac.insert(peer);
                self.active_in_nonbootstrap_connections += 1;
            }
        };
        self.request_dump()?;
        Ok(true)
    }
}

//to start alone RUST_BACKTRACE=1 cargo test peer_info_database -- --nocapture --test-threads=1
#[cfg(test)]
mod tests {
    use super::super::config::NetworkConfig;
    use super::*;
    use serial_test::serial;

    #[tokio::test]
    #[serial]
    async fn test_try_new_in_connection_in_connection_closed() {
        let mut network_config = example_network_config();
        network_config.target_out_nonbootstrap_connections = 5;
        let mut peers: HashMap<IpAddr, PeerInfo> = HashMap::new();

        // add peers
        // peer Ok, return
        let connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        peers.insert(connected_peers1.ip, connected_peers1);
        let mut connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)));
        connected_peers1.bootstrap = true;
        connected_peers1.banned = true;
        peers.insert(connected_peers1.ip, connected_peers1);

        let wakeup_interval = network_config.wakeup_interval;
        let (saver_watch_tx, mut saver_watch_rx) = watch::channel(peers.clone());

        let saver_join_handle = tokio::spawn(async move {
            loop {
                match saver_watch_rx.changed().await {
                    Ok(()) => (),
                    _ => break,
                }
            }
        });

        let mut db = PeerInfoDatabase {
            cfg: network_config,
            peers,
            saver_join_handle,
            saver_watch_tx,
            active_out_bootstrap_connection_attempts: 0,
            active_bootstrap_connections: 0,
            active_out_nonbootstrap_connection_attempts: 0,
            active_out_nonbootstrap_connections: 0,
            active_in_nonbootstrap_connections: 0,
            wakeup_interval,
            clock_compensation: 0,
        };

        // test with no connection attempt before
        let res = db.in_connection_closed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::CloseConnectionWithNoConnectionToClose(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)), ip_err);
        } else {
            assert!(false, "ToManyConnectionAttempt error not return");
        }

        let res = db
            .try_new_in_connection(&IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 0, 11)))
            .unwrap();
        assert!(!res, "not global ip not detected.");
        let res = db
            .try_new_in_connection(&IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)))
            .unwrap();
        assert!(!res, "local ip not detected.");

        let res = db
            .try_new_in_connection(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)))
            .unwrap();
        assert!(res, "in connection not accepted.");
        let res = db
            .try_new_in_connection(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)))
            .unwrap();
        assert!(!res, "banned peer not detected.");

        // test with a not connected peer
        let res = db.in_connection_closed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::CloseConnectionWithNoConnectionToClose(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)), ip_err);
        } else {
            assert!(false, "ToManyConnectionAttempt error not return");
        }

        // test with a not connected peer
        let res = db.in_connection_closed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 13)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::PeerInfoNotFoundError(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 13)), ip_err);
        } else {
            assert!(false, "PeerInfoNotFoundError error not return");
        }

        db.in_connection_closed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)))
            .unwrap();
        let res = db.in_connection_closed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::CloseConnectionWithNoConnectionToClose(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)), ip_err);
        } else {
            assert!(false, "ToManyConnectionAttempt error not return");
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_out_connection_attempt_failed() {
        let mut network_config = example_network_config();
        network_config.target_out_nonbootstrap_connections = 5;
        let mut peers: HashMap<IpAddr, PeerInfo> = HashMap::new();

        // add peers
        // peer Ok, return
        let connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        peers.insert(connected_peers1.ip, connected_peers1);
        let mut connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)));
        connected_peers1.bootstrap = true;
        connected_peers1.banned = true;
        peers.insert(connected_peers1.ip, connected_peers1);

        let wakeup_interval = network_config.wakeup_interval;
        let (saver_watch_tx, mut saver_watch_rx) = watch::channel(peers.clone());

        let saver_join_handle = tokio::spawn(async move {
            loop {
                match saver_watch_rx.changed().await {
                    Ok(()) => (),
                    _ => break,
                }
            }
        });

        let mut db = PeerInfoDatabase {
            cfg: network_config,
            peers,
            saver_join_handle,
            saver_watch_tx,
            active_out_bootstrap_connection_attempts: 0,
            active_bootstrap_connections: 0,
            active_out_nonbootstrap_connection_attempts: 0,
            active_out_nonbootstrap_connections: 0,
            active_in_nonbootstrap_connections: 0,
            wakeup_interval,
            clock_compensation: 0,
        };

        // test with no connection attempt before
        let res =
            db.out_connection_attempt_failed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::ToManyConnectionFailure(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)), ip_err);
        } else {
            println!("res: {:?}", res);
            assert!(false, "ToManyConnectionFailure error not return");
        }

        db.new_out_connection_attempt(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)))
            .unwrap();

        // peer not found.
        let res =
            db.out_connection_attempt_failed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 13)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::PeerInfoNotFoundError(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 13)), ip_err);
        } else {
            println!("res: {:?}", res);
            assert!(false, "PeerInfoNotFoundError error not return");
        }
        // peer with no attempt.
        let res =
            db.out_connection_attempt_failed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::ToManyConnectionFailure(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)), ip_err);
        } else {
            println!("res: {:?}", res);
            assert!(false, "ToManyConnectionFailure error not return");
        }

        // call ok.
        db.out_connection_attempt_failed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)))
            .expect("out_connection_attempt_failed failed");

        let res =
            db.out_connection_attempt_failed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::ToManyConnectionFailure(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)), ip_err);
        } else {
            assert!(false, "ToManyConnectionFailure error not return");
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_try_out_connection_attempt_success() {
        let mut network_config = example_network_config();
        network_config.target_out_nonbootstrap_connections = 5;
        let mut peers: HashMap<IpAddr, PeerInfo> = HashMap::new();

        // add peers
        // peer Ok, return
        let connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        peers.insert(connected_peers1.ip, connected_peers1);
        let mut connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)));
        connected_peers1.bootstrap = true;
        connected_peers1.banned = true;
        peers.insert(connected_peers1.ip, connected_peers1);

        let wakeup_interval = network_config.wakeup_interval;
        let (saver_watch_tx, mut saver_watch_rx) = watch::channel(peers.clone());

        let saver_join_handle = tokio::spawn(async move {
            loop {
                match saver_watch_rx.changed().await {
                    Ok(()) => (),
                    _ => break,
                }
            }
        });

        let mut db = PeerInfoDatabase {
            cfg: network_config,
            peers,
            saver_join_handle,
            saver_watch_tx,
            active_out_bootstrap_connection_attempts: 0,
            active_bootstrap_connections: 0,
            active_out_nonbootstrap_connection_attempts: 0,
            active_out_nonbootstrap_connections: 0,
            active_in_nonbootstrap_connections: 0,
            wakeup_interval,
            clock_compensation: 0,
        };

        // test with no connection attempt before
        let res = db.try_out_connection_attempt_success(&IpAddr::V4(std::net::Ipv4Addr::new(
            169, 202, 0, 11,
        )));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::ToManyConnectionAttempt(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)), ip_err);
        } else {
            assert!(false, "ToManyConnectionAttempt error not return");
        }

        db.new_out_connection_attempt(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)))
            .unwrap();

        // peer not found.
        let res = db.try_out_connection_attempt_success(&IpAddr::V4(std::net::Ipv4Addr::new(
            169, 202, 0, 13,
        )));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::PeerInfoNotFoundError(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 13)), ip_err);
        } else {
            println!("res: {:?}", res);
            assert!(false, "PeerInfoNotFoundError error not return");
        }

        let res = db
            .try_out_connection_attempt_success(&IpAddr::V4(std::net::Ipv4Addr::new(
                169, 202, 0, 11,
            )))
            .unwrap();
        assert!(res, "try_out_connection_attempt_success failed");

        let res = db.try_out_connection_attempt_success(&IpAddr::V4(std::net::Ipv4Addr::new(
            169, 202, 0, 12,
        )));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::ToManyConnectionAttempt(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)), ip_err);
        } else {
            assert!(false, "PeerInfoNotFoundError error not return");
        }

        db.new_out_connection_attempt(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)))
            .unwrap();
        let res = db
            .try_out_connection_attempt_success(&IpAddr::V4(std::net::Ipv4Addr::new(
                169, 202, 0, 12,
            )))
            .unwrap();
        assert!(!res, "try_out_connection_attempt_success not banned");
    }

    #[tokio::test]
    #[serial]
    async fn test_new_out_connection_closed() {
        let mut network_config = example_network_config();
        network_config.max_out_nonbootstrap_connection_attempts = 5;
        let mut peers: HashMap<IpAddr, PeerInfo> = HashMap::new();

        // add peers
        // peer Ok, return
        let connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        peers.insert(connected_peers1.ip, connected_peers1);
        let wakeup_interval = network_config.wakeup_interval;
        let (saver_watch_tx, mut saver_watch_rx) = watch::channel(peers.clone());
        let saver_join_handle = tokio::spawn(async move {
            loop {
                match saver_watch_rx.changed().await {
                    Ok(()) => (),
                    _ => break,
                }
            }
        });

        let mut db = PeerInfoDatabase {
            cfg: network_config,
            peers,
            saver_join_handle,
            saver_watch_tx,
            active_out_bootstrap_connection_attempts: 0,
            active_bootstrap_connections: 0,
            active_out_nonbootstrap_connection_attempts: 0,
            active_out_nonbootstrap_connections: 0,
            active_in_nonbootstrap_connections: 0,
            wakeup_interval,
            clock_compensation: 0,
        };

        //
        let res = db.out_connection_closed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::CloseConnectionWithNoConnectionToClose(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)), ip_err);
        } else {
            assert!(
                false,
                "CloseConnectionWithNoConnectionToClose error not return"
            );
        }

        // add a new connection attempt
        db.new_out_connection_attempt(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)))
            .unwrap();
        let res = db
            .try_out_connection_attempt_success(&IpAddr::V4(std::net::Ipv4Addr::new(
                169, 202, 0, 11,
            )))
            .unwrap();
        assert!(res, "try_out_connection_attempt_success failed");

        let res = db.out_connection_closed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::PeerInfoNotFoundError(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)), ip_err);
        } else {
            assert!(false, "PeerInfoNotFoundError error not return");
        }

        db.out_connection_closed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)))
            .unwrap();
        let res = db.out_connection_closed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::CloseConnectionWithNoConnectionToClose(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)), ip_err);
        } else {
            assert!(
                false,
                "CloseConnectionWithNoConnectionToClose error not return"
            );
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_new_out_connection_attempt() {
        let mut network_config = example_network_config();
        network_config.max_out_nonbootstrap_connection_attempts = 5;
        let mut peers: HashMap<IpAddr, PeerInfo> = HashMap::new();

        // add peers
        // peer Ok, return
        let connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        peers.insert(connected_peers1.ip, connected_peers1);
        let wakeup_interval = network_config.wakeup_interval;
        let (saver_watch_tx, _) = watch::channel(peers.clone());
        let saver_join_handle = tokio::spawn(async move {});

        let mut db = PeerInfoDatabase {
            cfg: network_config,
            peers,
            saver_join_handle,
            saver_watch_tx,
            active_out_bootstrap_connection_attempts: 0,
            active_bootstrap_connections: 0,
            active_out_nonbootstrap_connection_attempts: 0,
            active_out_nonbootstrap_connections: 0,
            active_in_nonbootstrap_connections: 0,
            wakeup_interval,
            clock_compensation: 0,
        };

        // test with no peers.
        let res =
            db.new_out_connection_attempt(&IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 0, 11)));
        if let Err(NetworkError::InvalidIpError(ip_err)) = res {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 0, 11)), ip_err);
        } else {
            assert!(false, "InvalidIpError not return");
        }

        let res =
            db.new_out_connection_attempt(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::PeerInfoNotFoundError(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)), ip_err);
        } else {
            assert!(false, "PeerInfoNotFoundError error not return");
        }

        (0..5).for_each(|_| {
            db.new_out_connection_attempt(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)))
                .unwrap()
        });
        let res =
            db.new_out_connection_attempt(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::ToManyConnectionAttempt(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)), ip_err);
        } else {
            assert!(false, "ToManyConnectionAttempt error not return");
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_get_advertisable_peer_ips() {
        let network_config = example_network_config();
        let mut peers: HashMap<IpAddr, PeerInfo> = HashMap::new();

        // add peers
        // peer Ok, return
        let connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        peers.insert(connected_peers1.ip, connected_peers1);
        // peer banned not return.
        let mut banned_host1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 23)));
        banned_host1.bootstrap = true;
        banned_host1.banned = true;
        banned_host1.last_alive = Some(UTime::now(0).unwrap().checked_sub(1000.into()).unwrap());
        peers.insert(banned_host1.ip, banned_host1);
        // peer not advertised, not return
        let mut connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 18)));
        connected_peers1.advertised = false;
        peers.insert(connected_peers1.ip, connected_peers1);
        // peer Ok, return
        let mut connected_peers2 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 13)));
        connected_peers2.last_alive = Some(UTime::now(0).unwrap().checked_sub(800.into()).unwrap());
        connected_peers2.last_failure =
            Some(UTime::now(0).unwrap().checked_sub(1000.into()).unwrap());
        peers.insert(connected_peers2.ip, connected_peers2);
        // peer Ok, connected return
        let mut connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 17)));
        connected_peers1.active_out_connections = 1;
        connected_peers1.last_alive = Some(UTime::now(0).unwrap().checked_sub(900.into()).unwrap());
        peers.insert(connected_peers1.ip, connected_peers1);
        // peer failure before alive but to early. return
        let mut connected_peers2 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 14)));
        connected_peers2.last_alive = Some(UTime::now(0).unwrap().checked_sub(800.into()).unwrap());
        connected_peers2.last_failure =
            Some(UTime::now(0).unwrap().checked_sub(2000.into()).unwrap());
        peers.insert(connected_peers2.ip, connected_peers2);

        let wakeup_interval = network_config.wakeup_interval;
        let (saver_watch_tx, _) = watch::channel(peers.clone());
        let saver_join_handle = tokio::spawn(async move {});

        let db = PeerInfoDatabase {
            cfg: network_config,
            peers,
            saver_join_handle,
            saver_watch_tx,
            active_out_bootstrap_connection_attempts: 0,
            active_bootstrap_connections: 0,
            active_out_nonbootstrap_connection_attempts: 0,
            active_out_nonbootstrap_connections: 0,
            active_in_nonbootstrap_connections: 0,
            wakeup_interval,
            clock_compensation: 0,
        };

        // test with no peers.
        let ip_list = db.get_advertisable_peer_ips();

        assert_eq!(5, ip_list.len());

        assert_eq!(
            IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)),
            ip_list[0]
        );
        assert_eq!(
            IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 14)),
            ip_list[1]
        );
        assert_eq!(
            IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 13)),
            ip_list[2]
        );
        assert_eq!(
            IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 17)),
            ip_list[3]
        );
        assert_eq!(
            IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)),
            ip_list[4]
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_get_out_connection_candidate_ips() {
        let network_config = example_network_config();
        let mut peers: HashMap<IpAddr, PeerInfo> = HashMap::new();

        // add peers
        // peer Ok, return
        let mut connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        connected_peers1.bootstrap = true;
        peers.insert(connected_peers1.ip, connected_peers1);
        // peer failure to early. not return
        let mut connected_peers2 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)));
        connected_peers2.last_failure =
            Some(UTime::now(0).unwrap().checked_sub(900.into()).unwrap());
        peers.insert(connected_peers2.ip, connected_peers2);
        // peer failure before alive but to early. return
        let mut connected_peers2 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 13)));
        connected_peers2.last_alive = Some(UTime::now(0).unwrap().checked_sub(900.into()).unwrap());
        connected_peers2.last_failure =
            Some(UTime::now(0).unwrap().checked_sub(1000.into()).unwrap());
        peers.insert(connected_peers2.ip, connected_peers2);
        // peer alive no failure. return
        let mut connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 14)));
        connected_peers1.last_alive =
            Some(UTime::now(0).unwrap().checked_sub(1000.into()).unwrap());
        peers.insert(connected_peers1.ip, connected_peers1);
        // peer banned not return.
        let mut banned_host1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 23)));
        banned_host1.bootstrap = true;
        banned_host1.banned = true;
        banned_host1.last_alive = Some(UTime::now(0).unwrap().checked_sub(1000.into()).unwrap());
        peers.insert(banned_host1.ip, banned_host1);
        // peer failure after alive not to early. return
        let mut connected_peers2 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 15)));
        connected_peers2.last_alive =
            Some(UTime::now(0).unwrap().checked_sub(12000.into()).unwrap());
        connected_peers2.last_failure =
            Some(UTime::now(0).unwrap().checked_sub(11000.into()).unwrap());
        peers.insert(connected_peers2.ip, connected_peers2);
        // peer failure after alive to early. not return
        let mut connected_peers2 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 16)));
        connected_peers2.last_alive =
            Some(UTime::now(0).unwrap().checked_sub(2000.into()).unwrap());
        connected_peers2.last_failure =
            Some(UTime::now(0).unwrap().checked_sub(1000.into()).unwrap());
        peers.insert(connected_peers2.ip, connected_peers2);
        // peer Ok, connected, not return
        let mut connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 17)));
        connected_peers1.active_out_connections = 1;
        peers.insert(connected_peers1.ip, connected_peers1);
        // peer Ok, not advertised, not return
        let mut connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 18)));
        connected_peers1.advertised = false;
        peers.insert(connected_peers1.ip, connected_peers1);

        let wakeup_interval = network_config.wakeup_interval;
        let (saver_watch_tx, _) = watch::channel(peers.clone());
        let saver_join_handle = tokio::spawn(async move {});

        let db = PeerInfoDatabase {
            cfg: network_config,
            peers,
            saver_join_handle,
            saver_watch_tx,
            active_out_bootstrap_connection_attempts: 0,
            active_bootstrap_connections: 0,
            active_out_nonbootstrap_connection_attempts: 0,
            active_out_nonbootstrap_connections: 0,
            active_in_nonbootstrap_connections: 0,
            wakeup_interval,
            clock_compensation: 0,
        };

        // test with no peers.
        let ip_list = db.get_out_connection_candidate_ips().unwrap();
        assert_eq!(4, ip_list.len());

        assert_eq!(
            IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)),
            ip_list[0]
        );
        assert_eq!(
            IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 14)),
            ip_list[1]
        );
        assert_eq!(
            IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 15)),
            ip_list[2]
        );
        assert_eq!(
            IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 13)),
            ip_list[3]
        );
    }

    fn default_peer_info_not_connected(ip: IpAddr) -> PeerInfo {
        PeerInfo {
            ip,
            banned: false,
            bootstrap: false,
            last_alive: None,
            last_failure: None,
            advertised: true,
            active_out_connection_attempts: 0,
            active_out_connections: 0,
            active_in_connections: 0,
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_cleanup_peers() {
        let mut network_config = example_network_config();
        network_config.max_banned_peers = 1;
        network_config.max_idle_peers = 1;
        let mut peers = HashMap::new();

        // Call with empty db.
        cleanup_peers(
            &network_config,
            &mut peers,
            None,
            0,
            network_config.ban_timeout,
        )
        .unwrap();
        assert!(peers.is_empty());

        let now = UTime::now(0).unwrap();

        let mut connected_peers1 =
            default_peer_info_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        connected_peers1.last_alive =
            Some(UTime::now(0).unwrap().checked_sub(1000.into()).unwrap());
        peers.insert(connected_peers1.ip, connected_peers1);

        let mut connected_peers2 =
            default_peer_info_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)));
        connected_peers2.last_alive = Some(UTime::now(0).unwrap().checked_sub(900.into()).unwrap());
        let same_connected_peer = connected_peers2;

        let non_global =
            default_peer_info_connected(IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 0, 10)));
        let same_host =
            default_peer_info_connected(IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)));

        let mut banned_host1 =
            default_peer_info_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 23)));
        banned_host1.bootstrap = false;
        banned_host1.banned = true;
        banned_host1.active_out_connections = 0;
        banned_host1.last_alive = Some(now.checked_sub(1000.into()).unwrap());
        banned_host1.last_failure = Some(now.checked_sub(2000.into()).unwrap());
        let mut banned_host2 =
            default_peer_info_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 24)));
        banned_host2.bootstrap = false;
        banned_host2.banned = true;
        banned_host2.active_out_connections = 0;
        banned_host2.last_alive = Some(now.checked_sub(900.into()).unwrap());
        banned_host2.last_failure = Some(now.checked_sub(2000.into()).unwrap());
        let mut banned_host3 =
            default_peer_info_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 25)));
        banned_host3.bootstrap = false;
        banned_host3.banned = true;
        banned_host3.last_alive = Some(now.checked_sub(900.into()).unwrap());
        banned_host3.last_failure = Some(now.checked_sub(2000.into()).unwrap());

        let mut advertised_host1 =
            default_peer_info_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 35)));
        advertised_host1.bootstrap = false;
        advertised_host1.advertised = true;
        advertised_host1.active_out_connections = 0;
        advertised_host1.last_alive =
            Some(UTime::now(0).unwrap().checked_sub(1000.into()).unwrap());
        let mut advertised_host2 =
            default_peer_info_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 36)));
        advertised_host2.bootstrap = false;
        advertised_host2.advertised = true;
        advertised_host2.active_out_connections = 0;
        advertised_host2.last_alive = Some(now.checked_sub(900.into()).unwrap());

        peers.insert(advertised_host1.ip, advertised_host1);
        peers.insert(banned_host1.ip, banned_host1);
        peers.insert(non_global.ip, non_global);
        peers.insert(same_connected_peer.ip, same_connected_peer);
        peers.insert(connected_peers2.ip, connected_peers2);
        peers.insert(connected_peers1.ip, connected_peers1);
        peers.insert(advertised_host2.ip, advertised_host2);
        peers.insert(same_host.ip, same_host);
        peers.insert(banned_host3.ip, banned_host3);
        peers.insert(banned_host2.ip, banned_host2);

        cleanup_peers(
            &network_config,
            &mut peers,
            None,
            0,
            network_config.ban_timeout,
        )
        .unwrap();

        assert!(peers.contains_key(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11))));
        assert!(peers.contains_key(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12))));

        assert!(peers.contains_key(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 23))));
        assert!(!peers.contains_key(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 24))));
        assert!(peers.contains_key(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 25))));

        assert!(!peers.contains_key(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 35))));
        assert!(peers.contains_key(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 36))));

        // test with advertised peers
        let advertised = vec![
            IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 0, 10)),
            IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 43)),
            IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)),
            IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 44)),
            IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)),
        ];

        network_config.max_advertise_length = 1;
        network_config.max_idle_peers = 5;

        cleanup_peers(
            &network_config,
            &mut peers,
            Some(&advertised),
            0,
            network_config.ban_timeout,
        )
        .unwrap();

        assert!(peers.contains_key(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 43))));
    }

    #[tokio::test]
    #[serial]
    async fn test() {
        let peer_db = peer_database_example(5);
        let p = peer_db.peers.values().next().unwrap();
        assert_eq!(p.is_active(), false);
    }
    fn default_peer_info_connected(ip: IpAddr) -> PeerInfo {
        PeerInfo {
            ip,
            banned: false,
            bootstrap: false,
            last_alive: None,
            last_failure: None,
            advertised: false,
            active_out_connection_attempts: 0,
            active_out_connections: 1,
            active_in_connections: 0,
        }
    }

    fn example_network_config() -> NetworkConfig {
        use std::net::{Ipv4Addr, SocketAddr};

        NetworkConfig {
            bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080),
            routable_ip: Some(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))),
            protocol_port: 0,
            connect_timeout: UTime::from(180_000),
            wakeup_interval: UTime::from(10_000),
            peers_file: std::path::PathBuf::new(),
            target_bootstrap_connections: 1,
            max_out_bootstrap_connection_attempts: 1,
            target_out_nonbootstrap_connections: 10,
            max_in_nonbootstrap_connections: 5,
            max_in_connections_per_ip: 2,
            max_out_nonbootstrap_connection_attempts: 15,
            max_idle_peers: 3,
            max_banned_peers: 3,
            max_advertise_length: 5,
            peers_file_dump_interval: UTime::from(10_000),
            max_message_size: 3 * 10241024,
            message_timeout: UTime::from(5000u64),
            ask_peer_list_interval: UTime::from(50000u64),
            private_key_file: std::path::PathBuf::new(),
            max_ask_blocks_per_message: 10,
            max_operations_per_message: 1024,
            max_endorsements_per_message: 1024,
            max_send_wait: UTime::from(100),
            ban_timeout: UTime::from(100_000_000),
        }
    }

    fn peer_database_example(peers_number: u32) -> PeerInfoDatabase {
        use rand::Rng;

        let mut rng = rand::thread_rng();

        let mut peers: HashMap<IpAddr, PeerInfo> = HashMap::new();
        for i in 0..peers_number {
            let ip: [u8; 4] = [rng.gen(), rng.gen(), rng.gen(), rng.gen()];
            let peer = PeerInfo {
                ip: IpAddr::from(ip),
                banned: (ip[0] % 5) == 0,
                bootstrap: (ip[1] % 2) == 0,
                last_alive: match i % 4 {
                    0 => None,
                    _ => Some(UTime::now(0).unwrap().checked_sub(50000.into()).unwrap()),
                },
                last_failure: match i % 5 {
                    0 => None,
                    _ => Some(UTime::now(0).unwrap().checked_sub(60000.into()).unwrap()),
                },
                advertised: (ip[2] % 2) == 0,
                active_out_connection_attempts: 0,
                active_out_connections: 0,
                active_in_connections: 0,
            };
            peers.insert(peer.ip, peer);
        }
        let cfg = example_network_config();
        let wakeup_interval = cfg.wakeup_interval;

        let (saver_watch_tx, _) = watch::channel(peers.clone());
        let saver_join_handle = tokio::spawn(async move {});
        PeerInfoDatabase {
            cfg,
            peers,
            saver_join_handle,
            saver_watch_tx,
            active_out_bootstrap_connection_attempts: 0,
            active_bootstrap_connections: 0,
            active_out_nonbootstrap_connection_attempts: 0,
            active_out_nonbootstrap_connections: 0,
            active_in_nonbootstrap_connections: 0,
            wakeup_interval,
            clock_compensation: 0,
        }
    }
}
