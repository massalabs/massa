// Copyright (c) 2022 MASSA LABS <info@massa.net>

use enum_map::EnumMap;
use itertools::Itertools;
use massa_logging::massa_trace;
use massa_models::constants::MAX_ADVERTISE_LENGTH;
use massa_network_exports::settings::PeerTypeConnectionConfig;
use massa_network_exports::ConnectionCount;
use massa_network_exports::NetworkConnectionErrorType;
use massa_network_exports::NetworkError;
use massa_network_exports::NetworkSettings;
use massa_network_exports::PeerInfo;
use massa_network_exports::PeerType;
use massa_time::MassaTime;
use serde_json::json;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::Path;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use tracing::{trace, warn};
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
    /// Connections count for each `PeerType`
    pub(crate) peer_types_connection_count: EnumMap<PeerType, ConnectionCount>,
    /// Every `wakeup_interval` we try to establish a connection with known inactive peers
    pub(crate) wakeup_interval: MassaTime,
    /// Clock compensation.
    pub(crate) clock_compensation: i64,
}

/// Saves advertised and non standard peers to a file.
///
/// # Arguments
/// * `peers`: peers to save
/// * `file_path`: path to the file
async fn dump_peers(
    peers: &HashMap<IpAddr, PeerInfo>,
    file_path: &Path,
) -> Result<(), NetworkError> {
    let peer_vec: Vec<_> = peers
        .values()
        .filter(|v| v.advertised || v.peer_type != PeerType::Standard || v.banned)
        .map(|peer| {
            json!({
                "ip": peer.ip,
                "banned": peer.banned,
                "peer_type": peer.peer_type,
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
/// provided by `NetworkConfig.ProtocolConfig`.
/// If `opt_new_peers` is provided, adds its contents as well.
///
/// Note: only standard non-active peers are counted when clipping to size limits.
///
/// Arguments :
/// * `cfg`: `NetworkSettings`
/// * `peers`: peers to clean up
/// * `opt_new_peers`: optional peers to add to the database
/// * `clock_compensation`: to be sync with server time
/// * `ban_timeout`: after that time we forget we banned a peer
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
            .map(|ip| ip.to_canonical())
            .unique()
            .filter(|&ip| {
                if let Some(mut p) = peers.get_mut(&ip) {
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
                    if ip == our_ip.to_canonical() {
                        return false;
                    }
                }
                true
            })
            .take(MAX_ADVERTISE_LENGTH as usize)
            .map(|ip| PeerInfo::new(ip, true))
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
            if ip == our_ip.to_canonical() {
                continue;
            }
        }
        if p.peer_type != Default::default() || p.is_active() {
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
    /// Creates new `PeerInfoDatabase` from `NetworkConfig`.
    /// will only emit a warning if peers dumping failed.
    ///
    /// # Argument
    /// * `cfg`: network configuration
    /// * `clock_compensation`: sync with server
    pub async fn new(cfg: &NetworkSettings, clock_compensation: i64) -> Result<Self, NetworkError> {
        // wakeup interval
        let wakeup_interval = cfg.wakeup_interval;

        // load from initial file
        let mut peers = serde_json::from_str::<Vec<PeerInfo>>(
            &tokio::fs::read_to_string(&cfg.initial_peers_file).await?,
        )?
        .into_iter()
        .map(|mut p| {
            p.cleanup();
            (p.ip, p)
        })
        .collect::<HashMap<IpAddr, PeerInfo>>();
        if cfg.peers_file.is_file() {
            peers.extend(
                // previously known peers
                serde_json::from_str::<Vec<PeerInfo>>(
                    &tokio::fs::read_to_string(&cfg.peers_file).await?,
                )?
                .into_iter()
                .map(|mut p| {
                    p.cleanup();
                    (p.ip, p)
                }),
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
            wakeup_interval,
            clock_compensation,
            peer_types_connection_count: EnumMap::default(),
        })
    }

    /// Cleanly closes `peerInfoDatabase`, performing one last peer dump.
    /// A warning is raised on dump failure.
    pub async fn stop(self) -> Result<(), NetworkError> {
        drop(self.saver_watch_tx);
        self.saver_join_handle.await?;
        if let Err(e) = dump_peers(&self.peers, &self.network_settings.peers_file).await {
            warn!("could not dump peers to file: {}", e);
        }
        Ok(())
    }

    //////////////////////
    // aggregated stats //
    //////////////////////

    /// total in connection count, considering all peer types
    /// quite similar to `get_out_connection_count`
    /// todo `https://github.com/massalabs/massa/issues/2319`
    #[inline]
    pub fn get_in_connection_count(&self) -> u64 {
        self.peer_types_connection_count
            .values()
            .fold(0, |acc, connection_count| {
                acc + (connection_count.active_in_connections as u64)
            })
    }

    /// total out connections count, considering all peer types
    /// quite similar to `get_in_connection_count`
    /// todo `https://github.com/massalabs/massa/issues/2319`
    #[inline]
    pub fn get_out_connection_count(&self) -> u64 {
        self.peer_types_connection_count
            .values()
            .fold(0, |acc, connection_count| {
                acc + (connection_count.active_out_connections as u64)
            })
    }

    ///////////////////////
    // hard disk storage //
    ///////////////////////

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

    /// Merges `new_peers` with our peers using the `cleanup_peers` function.
    /// A dump is requested afterwards.
    ///
    /// # Argument
    /// `new_peers`: peers we are trying to merge
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

    ////////////////////////////////
    // high level peer management //
    ////////////////////////////////

    /// Unban a list of ip
    pub fn unban(&mut self, ips: Vec<IpAddr>) -> Result<(), NetworkError> {
        let mut update_happened = false;
        for ip in ips.into_iter() {
            let ip = ip.to_canonical();
            if let Some(peer) = self.peers.get_mut(&ip) {
                update_happened = update_happened || peer.banned;
                peer.banned = false;
            }
        }
        self.update()?;
        if update_happened {
            self.request_dump()?
        }
        Ok(())
    }

    pub async fn whitelist(&mut self, ips: Vec<IpAddr>) -> Result<(), NetworkError> {
        for ip in ips.into_iter() {
            let ip = ip.to_canonical();
            let old_pt = if let Some(peer) = self.peers.get_mut(&ip) {
                let pt = peer.peer_type;
                if pt == PeerType::WhiteListed {
                    continue;
                }
                peer.peer_type = PeerType::WhiteListed;
                pt
            } else {
                let mut p = PeerInfo::new(ip, false);
                p.peer_type = PeerType::WhiteListed;
                self.peers.insert(ip, p);
                continue;
            };
            // update global connection counts by peer type
            let peer = *self.peers.get(&ip).unwrap(); // filled just before
            if peer.active_out_connection_attempts > 0 {
                self.decrease_global_active_out_connection_attempt_count(old_pt, &ip)?;
                self.increase_global_active_out_connection_attempt_count(
                    PeerType::WhiteListed,
                    &ip,
                )?
            }
            if peer.active_out_connections > 0 {
                self.decrease_global_active_out_connection_count(old_pt, &ip)?;
                self.increase_global_active_out_connection_count(PeerType::WhiteListed)?
            }
            if peer.active_in_connections > 0 {
                self.decrease_global_active_in_connection_count(old_pt, &ip)?;
                self.increase_global_active_in_connection_count(PeerType::WhiteListed)?
            }
        }
        self.update()
    }

    pub async fn remove_from_whitelist(&mut self, ips: Vec<IpAddr>) -> Result<(), NetworkError> {
        for ip in ips.into_iter() {
            let ip = ip.to_canonical();
            let old_pt = if let Some(peer) = self.peers.get_mut(&ip) {
                let old = peer.peer_type;
                peer.peer_type = Default::default();
                old
            } else {
                return Ok(());
            };

            if old_pt != Default::default() {
                // update global connection counts by peer type
                // as the peer isn't whitelist anymore
                let peer = *self.peers.get(&ip).unwrap(); // filled just before
                if peer.active_out_connection_attempts > 0 {
                    self.decrease_global_active_out_connection_attempt_count(old_pt, &ip)?;
                    self.increase_global_active_out_connection_attempt_count(
                        Default::default(),
                        &ip,
                    )?
                }
                if peer.active_out_connections > 0 {
                    self.decrease_global_active_out_connection_count(old_pt, &ip)?;
                    self.increase_global_active_out_connection_count(Default::default())?
                }
                if peer.active_in_connections > 0 {
                    self.decrease_global_active_in_connection_count(old_pt, &ip)?;
                    self.increase_global_active_in_connection_count(Default::default())?
                }
            }
        }
        self.update()
    }

    /// Acknowledges a new out connection attempt to ip.
    ///
    /// # Argument
    /// `ip`: `IpAddr` we are now connected to
    pub fn new_out_connection_attempt(&mut self, ip: &IpAddr) -> Result<(), NetworkError> {
        let ip = ip.to_canonical();
        if !ip.is_global() {
            return Err(NetworkError::InvalidIpError(ip));
        }
        let peer_type = if let Some(peer) = self.peers.get(&ip) {
            if self.can_try_new_out_connection(peer.peer_type) {
                // Can unwrap because we checked above that there is a peer.
                let peer = self.peers.get_mut(&ip).unwrap();
                peer.active_out_connection_attempts += 1;
                Ok(peer.peer_type)
            } else {
                Err(NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::TooManyConnectionAttempts(ip),
                ))
            }
        } else if self.can_try_new_out_connection(Default::default()) {
            let mut peer = PeerInfo::new(ip, false);
            peer.active_out_connection_attempts += 1;
            self.peers.insert(ip, peer);
            Ok(peer.peer_type)
        } else {
            Err(NetworkError::PeerConnectionError(
                NetworkConnectionErrorType::TooManyConnectionAttempts(ip),
            ))
        }?;
        self.increase_global_active_out_connection_attempt_count(peer_type, &ip)?;
        self.update()
    }

    /// Sets the peer status as alive.
    /// Requests a subsequent dump.
    ///
    /// # Argument
    /// * ip : ip address of the considered peer.
    pub fn peer_alive(&mut self, ip: &IpAddr) -> Result<(), NetworkError> {
        let ip = ip.to_canonical();
        self.peers
            .get_mut(&ip)
            .ok_or({
                NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::PeerInfoNotFoundError(ip),
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
        let ip = ip.to_canonical();
        self.peers
            .get_mut(&ip)
            .ok_or({
                NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::PeerInfoNotFoundError(ip),
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
        let ip = ip.to_canonical();
        let peer = self
            .peers
            .entry(ip)
            .or_insert_with(|| PeerInfo::new(ip, false));
        peer.last_failure = Some(MassaTime::compensated_now(self.clock_compensation)?);
        if !peer.banned {
            peer.banned = true;
            if !peer.is_active() {
                self.update()?
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
        let ip = ip.to_canonical();
        let peer_type = {
            let peer = self.peers.get(&ip).ok_or({
                NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::PeerInfoNotFoundError(ip),
                )
            })?;
            if peer.active_out_connections == 0
                || !self.can_remove_active_out_connection_count(peer.peer_type)
            {
                return Err(NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::CloseConnectionWithNoConnectionToClose(ip),
                ));
            }
            let peer = self.peers.get_mut(&ip).ok_or({
                NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::PeerInfoNotFoundError(ip),
                )
            })?;
            peer.active_out_connections -= 1;
            let peer_type = peer.peer_type;
            if !peer.is_active() && peer.peer_type == Default::default() {
                self.update()?;
                self.request_dump()?;
            }
            peer_type
        };
        self.decrease_global_active_out_connection_count(peer_type, &ip)?;
        Ok(())
    }

    /// Notifies that an inbound connection is closed.
    ///
    /// If the peer is not active nor bootstrap
    /// peers are cleaned up and a dump is requested.
    ///
    /// # Argument
    /// * ip : ip address of the considered peer.
    pub fn in_connection_closed(&mut self, ip: &IpAddr) -> Result<(), NetworkError> {
        let ip = ip.to_canonical();
        let peer_type = {
            let peer = self.peers.get(&ip).ok_or({
                NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::PeerInfoNotFoundError(ip),
                )
            })?;
            if peer.active_in_connections == 0
                || !self.can_decrease_global_active_in_connection_count(peer.peer_type)
            {
                return Err(NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::CloseConnectionWithNoConnectionToClose(ip),
                ));
            }
            let peer = self.peers.get_mut(&ip).ok_or({
                NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::PeerInfoNotFoundError(ip),
                )
            })?;
            peer.active_in_connections -= 1;
            let peer_type = peer.peer_type;
            if !peer.is_active() && peer.peer_type == PeerType::Standard {
                self.update()?;
                self.request_dump()?;
            }
            peer_type
        };

        self.decrease_global_active_in_connection_count(peer_type, &ip)?;
        Ok(())
    }

    /// An out connection attempt succeeded.
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
        let ip = ip.to_canonical();
        // a connection attempt succeeded
        // remove out connection attempt and add out connection
        let peer_type = self.get_peer_type(&ip).ok_or({
            NetworkError::PeerConnectionError(NetworkConnectionErrorType::PeerInfoNotFoundError(ip))
        })?;

        // have we reached target yet ?
        if self.is_target_out_connection_count_reached(peer_type) {
            return Ok(false);
        }

        self.decrease_global_active_out_connection_attempt_count(peer_type, &ip)?;

        let peer_type = {
            let peer = self.peers.get(&ip).ok_or({
                NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::PeerInfoNotFoundError(ip),
                )
            })?;
            if peer.active_out_connection_attempts == 0 {
                return Err(NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::TooManyConnectionAttempts(ip),
                ));
            }
            let peer = self.peers.get_mut(&ip).ok_or({
                NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::PeerInfoNotFoundError(ip),
                )
            })?;
            peer.active_out_connection_attempts -= 1;
            peer.advertised = true; // we just connected to it. Assume advertised.

            if peer.banned {
                peer.last_failure = Some(MassaTime::compensated_now(self.clock_compensation)?);
                if !peer.is_active() && peer.peer_type == Default::default() {
                    self.update()?;
                }
                self.request_dump()?;
                return Ok(false);
            }
            peer.active_out_connections += 1;
            peer.peer_type
        };
        self.increase_global_active_out_connection_count(peer_type)?;
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
        let ip = ip.to_canonical();
        let peer_type = {
            let peer = self
                .peers
                .get(&ip)
                .ok_or(NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::PeerInfoNotFoundError(ip),
                ))?;
            if peer.active_out_connection_attempts == 0
                || !self.can_remove_new_out_connection_attempt(peer.peer_type)
            {
                return Err(NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::TooManyConnectionFailure(ip),
                ));
            }
            let peer = self
                .peers
                .get_mut(&ip)
                .ok_or(NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::PeerInfoNotFoundError(ip),
                ))?;
            peer.active_out_connection_attempts -= 1;
            peer.last_failure = Some(MassaTime::compensated_now(self.clock_compensation)?);
            let pt = peer.peer_type;
            if !peer.is_active() && peer.peer_type == PeerType::Standard {
                self.update()?;
            }
            pt
        };
        self.decrease_global_active_out_connection_attempt_count(peer_type, &ip)?;
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
        let ip = ip.to_canonical();
        // try to create a new input connection, return false if no slots
        if !ip.is_global() || self.network_settings.max_in_connections_per_ip == 0 {
            return Err(NetworkError::PeerConnectionError(
                NetworkConnectionErrorType::MaxPeersConnectionReached(ip),
            ));
        }
        if let Some(our_ip) = self.network_settings.routable_ip {
            // avoid our own IP
            if ip == our_ip.to_canonical() {
                warn!("incoming connection from our own IP");
                return Err(NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::SelfConnection,
                ));
            }
        }

        let peer_type = self
            .peers
            .entry(ip)
            .or_insert_with(|| PeerInfo::new(ip, false))
            .peer_type;

        // we need to first check if there is a global slot available
        if self.is_max_in_connection_count_reached(peer_type) {
            return Err(NetworkError::PeerConnectionError(
                NetworkConnectionErrorType::MaxPeersConnectionReached(ip),
            ));
        }

        let peer_type = {
            let peer = self.peers.get_mut(&ip).ok_or({
                NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::PeerInfoNotFoundError(ip),
                )
            })?; // peer was inserted just before

            // is there a attempt slot available
            if peer.banned {
                massa_trace!("in_connection_refused_peer_banned", {"ip": peer.ip});
                peer.last_failure = Some(MassaTime::compensated_now(self.clock_compensation)?);
                self.request_dump()?;
                return Err(NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::BannedPeerTryingToConnect(ip),
                ));
            } else if peer.active_in_connections >= self.network_settings.max_in_connections_per_ip
            {
                self.request_dump()?;
                return Err(NetworkError::PeerConnectionError(
                    NetworkConnectionErrorType::MaxPeersConnectionReached(ip),
                ));
            } else {
                peer.active_in_connections += 1;
            }
            peer.peer_type
        };

        self.increase_global_active_in_connection_count(peer_type)?;
        self.request_dump()?;
        Ok(())
    }

    ////////////////////
    // public getters //
    ////////////////////

    /// Sorts peers by `( last_failure, rev(last_success) )`
    /// and returns as many peers as there are available slots to attempt outgoing connections to.
    pub fn get_out_connection_candidate_ips(&self) -> Result<Vec<IpAddr>, NetworkError> {
        let mut connections = vec![];
        let mut peer_types: Vec<PeerType> = self
            .peer_types_connection_count
            .iter()
            .map(|(peer_type, _)| peer_type)
            .collect();
        peer_types.sort_by_key(|&peer_type| Reverse(peer_type));
        for &peer_type in peer_types.iter() {
            connections.append(&mut self.get_out_connection_candidate_ips_for_type(
                peer_type,
                &self.peer_types_connection_count[peer_type],
                &self.network_settings.peer_types_config[peer_type],
            )?);
        }
        Ok(connections)
    }

    /// returns Hashmap of `IpAddrs` -> `PeerInfo`
    pub fn get_peers(&self) -> &HashMap<IpAddr, PeerInfo> {
        &self.peers
    }

    /// Returns a vector of advertisable `IpAddr` sorted by `( last_failure, rev(last_success) )`
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
            sorted_ips.insert(0, our_ip.to_canonical());
            sorted_ips.truncate(MAX_ADVERTISE_LENGTH as usize);
        }
        sorted_ips
    }

    //////////////////////////////
    // per peer type management //
    //////////////////////////////

    fn get_available_out_connection_attempts_for_peer_type(&self, peer_type: PeerType) -> usize {
        self.peer_types_connection_count[peer_type].get_available_out_connection_attempts(
            &self.network_settings.peer_types_config[peer_type],
        )
    }

    fn is_target_out_connection_count_reached(&self, peer_type: PeerType) -> bool {
        self.peer_types_connection_count[peer_type].active_out_connections
            >= self.network_settings.peer_types_config[peer_type].target_out_connections
    }

    fn is_max_in_connection_count_reached(&self, peer_type: PeerType) -> bool {
        self.peer_types_connection_count[peer_type].active_in_connections
            >= self.network_settings.peer_types_config[peer_type].max_in_connections
    }

    /// Get ips we want to connect to for a given peer type
    ///
    /// # Arguments
    /// * `peer_type`: which type to consider
    /// * `count`: what is the current connection count for that type
    /// * `cfg`: settings for that peer type
    ///
    /// Returns an iterator
    fn get_out_connection_candidate_ips_for_type(
        &self,
        peer_type: PeerType,
        count: &ConnectionCount,
        cfg: &PeerTypeConnectionConfig,
    ) -> Result<Vec<IpAddr>, NetworkError> {
        let available_slots = count.get_available_out_connection_attempts(cfg);
        let now = MassaTime::compensated_now(self.clock_compensation)?;
        let f = move |p: &&PeerInfo| {
            if p.peer_type != peer_type || !p.advertised || p.is_active() || p.banned {
                return false;
            }
            p.is_peer_ready(self.wakeup_interval, now)
        };
        let mut res: Vec<_> = self
            .peers
            .values()
            .filter(f)
            .take(available_slots)
            .collect();
        res.sort_unstable_by_key(|&p| (p.last_failure, std::cmp::Reverse(p.last_alive)));
        Ok(res.into_iter().map(|p| p.ip).collect())
    }

    fn get_peer_type(&self, ip: &IpAddr) -> Option<PeerType> {
        Some(self.peers.get(ip)?.peer_type)
    }

    fn can_try_new_out_connection(&self, peer_type: PeerType) -> bool {
        self.get_available_out_connection_attempts_for_peer_type(peer_type) != 0
    }

    fn can_remove_new_out_connection_attempt(&self, peer_type: PeerType) -> bool {
        self.get_global_active_out_connection_attempt_count(peer_type) != 0
    }

    fn increase_global_active_out_connection_attempt_count(
        &mut self,
        peer_type: PeerType,
        ip: &IpAddr,
    ) -> Result<(), NetworkError> {
        if !self.can_try_new_out_connection(peer_type) {
            return Err(NetworkError::PeerConnectionError(
                NetworkConnectionErrorType::TooManyConnectionAttempts(*ip),
            ));
        }
        self.peer_types_connection_count[peer_type].active_out_connection_attempts += 1;
        Ok(())
    }

    fn decrease_global_active_out_connection_attempt_count(
        &mut self,
        peer_type: PeerType,
        ip: &IpAddr,
    ) -> Result<(), NetworkError> {
        if !self.can_remove_new_out_connection_attempt(peer_type) {
            return Err(NetworkError::PeerConnectionError(
                NetworkConnectionErrorType::TooManyConnectionAttempts(*ip),
            ));
        }
        self.peer_types_connection_count[peer_type].active_out_connection_attempts -= 1;
        Ok(())
    }

    fn can_remove_active_out_connection_count(&self, peer_type: PeerType) -> bool {
        self.get_global_active_out_connection_count(peer_type) != 0
    }

    fn decrease_global_active_out_connection_count(
        &mut self,
        peer_type: PeerType,
        ip: &IpAddr,
    ) -> Result<(), NetworkError> {
        if !self.can_remove_active_out_connection_count(peer_type) {
            return Err(NetworkError::PeerConnectionError(
                NetworkConnectionErrorType::CloseConnectionWithNoConnectionToClose(*ip),
            ));
        }
        self.peer_types_connection_count[peer_type].active_out_connections -= 1;
        Ok(())
    }

    fn increase_global_active_out_connection_count(
        &mut self,
        peer_type: PeerType,
    ) -> Result<(), NetworkError> {
        self.peer_types_connection_count[peer_type].active_out_connections += 1;
        Ok(())
    }

    fn can_decrease_global_active_in_connection_count(&self, peer_type: PeerType) -> bool {
        self.get_global_active_in_connection_count(peer_type) != 0
    }

    fn decrease_global_active_in_connection_count(
        &mut self,
        peer_type: PeerType,
        ip: &IpAddr,
    ) -> Result<(), NetworkError> {
        if !self.can_decrease_global_active_in_connection_count(peer_type) {
            return Err(NetworkError::PeerConnectionError(
                NetworkConnectionErrorType::CloseConnectionWithNoConnectionToClose(*ip),
            ));
        }
        self.peer_types_connection_count[peer_type].active_in_connections -= 1;
        Ok(())
    }

    fn increase_global_active_in_connection_count(
        &mut self,
        peer_type: PeerType,
    ) -> Result<(), NetworkError> {
        self.peer_types_connection_count[peer_type].active_in_connections += 1;
        Ok(())
    }

    /// similar to `get_global_active_out_connection_count` and `get_global_active_in_connection_count`
    /// todo `https://github.com/massalabs/massa/issues/2319`
    #[inline]
    fn get_global_active_out_connection_attempt_count(&self, peer_type: PeerType) -> usize {
        self.peer_types_connection_count[peer_type].active_out_connection_attempts
    }

    /// similar to `get_global_active_out_connection_attempt_count` and `get_global_active_in_connection_count`
    /// todo `https://github.com/massalabs/massa/issues/2319`
    #[inline]
    fn get_global_active_out_connection_count(&self, peer_type: PeerType) -> usize {
        self.peer_types_connection_count[peer_type].active_out_connections
    }

    /// similar to `get_global_active_out_connection_count` and `get_global_active_out_connection_attempt_count`
    /// todo `https://github.com/massalabs/massa/issues/2319`
    #[inline]
    fn get_global_active_in_connection_count(&self, peer_type: PeerType) -> usize {
        self.peer_types_connection_count[peer_type].active_in_connections
    }
}
