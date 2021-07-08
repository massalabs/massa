use super::config::NetworkConfig;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use log::warn;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::net::IpAddr;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};

type BoxResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
pub struct PeerInfo {
    pub ip: IpAddr,
    pub banned: bool,
    pub bootstrap: bool,
    pub last_alive: Option<DateTime<Utc>>,
    pub last_failure: Option<DateTime<Utc>>,
    pub advertised: bool,

    #[serde(skip, default = "usize::default")]
    pub active_out_connection_attempts: usize,
    #[serde(skip, default = "usize::default")]
    pub active_out_connections: usize,
    #[serde(skip, default = "usize::default")]
    pub active_in_connections: usize,
}

impl PeerInfo {
    /// true if there is at least one connection attempt /
    ///  one active connection in either direction
    /// with this peer
    fn is_active(&self) -> bool {
        self.active_out_connection_attempts > 0
            || self.active_out_connections > 0
            || self.active_in_connections > 0
    }
}

pub struct PeerInfoDatabase {
    cfg: NetworkConfig,
    peers: HashMap<IpAddr, PeerInfo>,
    saver_join_handle: JoinHandle<()>,
    saver_watch_tx: watch::Sender<HashMap<IpAddr, PeerInfo>>,
    active_out_connection_attempts: usize,
    active_out_connections: usize,
    active_in_connections: usize,
}

/// Saves banned, advertised and bootstrap peers to a file.
/// Can return an error if the writing fails.
async fn dump_peers(
    peers: &HashMap<IpAddr, PeerInfo>,
    file_path: &std::path::PathBuf,
) -> BoxResult<()> {
    let peer_vec: Vec<PeerInfo> = peers
        .values()
        .filter(|v| v.banned || v.advertised || v.bootstrap)
        .cloned()
        .collect();
    tokio::fs::write(file_path, serde_json::to_string_pretty(&peer_vec)?).await?;
    Ok(())
}

/// Cleans up the peer database using max values
/// provided by NetworkConfig.ProtocolConfig.
/// If opt_new_peers is provided, adds its contents as well.
///
/// Note: only non-active, non-bootstrap peers are counted when clipping to size limits.
fn cleanup_peers(
    cfg: &NetworkConfig,
    peers: &mut HashMap<IpAddr, PeerInfo>,
    opt_new_peers: Option<&Vec<IpAddr>>,
) {
    // filter and map new peers, remove duplicates
    let mut res_new_peers: Vec<PeerInfo> = if let Some(new_peers) = opt_new_peers {
        new_peers
            .iter()
            .unique()
            .take(cfg.max_advertise_length)
            .filter_map(|&ip| {
                if let Some(mut p) = peers.get_mut(&ip) {
                    // avoid already-known IPs, but mark them as advertised
                    p.advertised = true;
                    return None;
                }
                if !ip.is_global() {
                    // avoid non-global IPs
                    return None;
                }
                if let Some(our_ip) = cfg.routable_ip {
                    // avoid our own IP
                    if ip == our_ip {
                        return None;
                    }
                }
                Some(PeerInfo {
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
    banned_peers.sort_unstable_by_key(|&p| (std::cmp::Reverse(p.last_failure), p.last_alive));
    banned_peers.truncate(cfg.max_banned_peers);

    // gather everything back
    peers.extend(keep_peers.into_iter().map(|p| (p.ip, p)));
    peers.extend(banned_peers.into_iter().map(|p| (p.ip, p)));
    peers.extend(idle_peers.into_iter().map(|p| (p.ip, p)));
}

impl PeerInfoDatabase {
    /// Creates new peerInfoDatabase from NetworkConfig.
    /// Can fail reading the file containing peers.
    /// will only emit a warning if peers dumping failed.
    pub async fn new(cfg: &NetworkConfig) -> BoxResult<Self> {
        // load from file
        let mut peers = serde_json::from_str::<Vec<PeerInfo>>(
            &tokio::fs::read_to_string(&cfg.peers_file).await?,
        )?
        .into_iter()
        .map(|p| (p.ip, p))
        .collect::<HashMap<IpAddr, PeerInfo>>();

        // cleanup
        cleanup_peers(&cfg, &mut peers, None);

        // setup saver
        let peers_file = cfg.peers_file.clone();
        let peers_file_dump_interval = cfg.peers_file_dump_interval;
        let (saver_watch_tx, mut saver_watch_rx) = watch::channel(peers.clone());
        let saver_join_handle = tokio::spawn(async move {
            let mut delay = sleep(Duration::from_secs_f32(0.0));
            let mut pending: Option<HashMap<IpAddr, PeerInfo>> = None;
            loop {
                tokio::select! {
                    opt_p = saver_watch_rx.changed() => match opt_p {
                        Ok(()) => {
                            pending = Some(saver_watch_rx.borrow().clone());
                            if !delay.is_elapsed() {
                                continue;
                            }
                            if let Err(e) = dump_peers(pending.as_ref().unwrap(), &peers_file).await {
                                warn!("could not dump peers to file: {}", e);
                            } else {
                                pending = None;
                            }
                            delay = sleep(peers_file_dump_interval);
                        },
                        _ => break
                    },
                    _ = &mut delay => {
                        if let Some(ref p) = pending {
                            if let Err(e) = dump_peers(&p, &peers_file).await {
                                warn!("could not dump peers to file: {}", e);
                                continue;
                            }
                            pending = None;
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
            active_out_connection_attempts: 0,
            active_out_connections: 0,
            active_in_connections: 0,
        })
    }

    /// Request peers dump to file
    fn request_dump(&self) {
        self.saver_watch_tx
            .send(self.peers.clone())
            .expect("peer database saver task disappeared");
    }

    /// Cleanly closes peerInfoDatabase, performing one last peer dump.
    /// A warining is raised on dump failure.
    pub async fn stop(self) {
        drop(self.saver_watch_tx);
        self.saver_join_handle
            .await
            .expect("could not join peer database saver");
        if let Err(e) = dump_peers(&self.peers, &self.cfg.peers_file).await {
            warn!("could not dump peers to file: {}", e);
        }
    }

    /// Gets avaible out connection attempts
    /// accordig to NeworkConfig and current connections and connection attempts.
    pub fn get_available_out_connection_attempts(&self) -> usize {
        std::cmp::min(
            self.cfg
                .target_out_connections
                .saturating_sub(self.active_out_connection_attempts)
                .saturating_sub(self.active_out_connections),
            self.cfg
                .max_out_connnection_attempts
                .saturating_sub(self.active_out_connection_attempts),
        )
    }

    /// Sorts peers by ( last_failure, rev(last_success) )
    /// and returns as many peers as there are avaible slots to attempt outgoing connections to.
    pub fn get_out_connection_candidate_ips(&self) -> Vec<IpAddr> {
        /*
            get_connect_candidate_ips must return the full sorted list where:
                advertised && !banned && out_connection_attempts==0 && out_connections==0 && in_connections=0
                sorted_by = ( last_failure, rev(last_success) )
        */
        let available_slots = self.get_available_out_connection_attempts();
        if available_slots == 0 {
            return Vec::new();
        }
        let mut sorted_peers: Vec<PeerInfo> = self
            .peers
            .values()
            .filter(|&p| {
                if !(p.advertised && !p.banned && !p.is_active()) {
                    return false;
                }
                if let Some(last_failure) = p.last_failure {
                    if let Some(last_alive) = p.last_alive {
                        if last_alive > last_failure {
                            return true;
                        }
                    }
                    return (Utc::now() - last_failure)
                        .to_std()
                        .expect("chrono duration unsupported by std")
                        > self.cfg.wakeup_interval;
                }
                true
            })
            .copied()
            .collect();
        sorted_peers.sort_unstable_by_key(|&p| (p.last_failure, std::cmp::Reverse(p.last_alive)));
        sorted_peers
            .into_iter()
            .take(available_slots)
            .map(|p| p.ip)
            .collect()
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
            .take(self.cfg.max_advertise_length)
            .map(|p| p.ip)
            .collect();
        if let Some(our_ip) = self.cfg.routable_ip {
            sorted_ips.insert(0, our_ip);
            sorted_ips.truncate(self.cfg.max_advertise_length);
        }
        sorted_ips
    }

    /// Acknowledges a new out connection attempt to ip.
    ///
    /// Panics if :
    /// - target ip is not global
    /// - there are too many out connection attempts
    /// - ip does not match with a known peer
    pub fn new_out_connection_attempt(&mut self, ip: &IpAddr) {
        if !ip.is_global() {
            panic!("target IP is not global");
        }
        if self.get_available_out_connection_attempts() == 0 {
            panic!("too many out connection attempts");
        }
        self.peers
            .get_mut(&ip)
            .expect("attempting connection towards unknown peer")
            .active_out_connection_attempts += 1;
        self.active_out_connection_attempts += 1;
    }

    /// Merges new_peers with our peers using the cleanup_peers function.
    /// A dump is requested afterwards.
    pub fn merge_candidate_peers(&mut self, new_peers: &Vec<IpAddr>) {
        if new_peers.is_empty() {
            return;
        }
        cleanup_peers(&self.cfg, &mut self.peers, Some(&new_peers));
        self.request_dump();
    }

    /// Sets the peer status as alive.
    /// Panics if ip does not match a known peer.
    /// Requests a subsequent dump.
    pub fn peer_alive(&mut self, ip: &IpAddr) {
        self.peers
            .get_mut(&ip)
            .expect("unknown peer alive")
            .last_alive = Some(Utc::now());
        self.request_dump();
    }

    /// Sets the peer status as failed.
    /// Panics if the peer is unknown.
    /// Requests a dump.
    pub fn peer_failed(&mut self, ip: &IpAddr) {
        self.peers
            .get_mut(&ip)
            .expect("unknown peer failed")
            .last_failure = Some(Utc::now());
        self.request_dump();
    }

    /// Sets that the peer is banned now.
    /// Panics if the ip does not match an unknown peer.
    /// If the peer is not active, the database is cleaned up.
    /// A dump is requested.
    pub fn peer_banned(&mut self, ip: &IpAddr) {
        let peer = self.peers.get_mut(&ip).expect("unknown peer failed");
        peer.last_failure = Some(Utc::now());
        if !peer.banned {
            peer.banned = true;
            if !peer.is_active() && !peer.bootstrap {
                cleanup_peers(&self.cfg, &mut self.peers, None);
            }
        }
        self.request_dump();
    }

    /// Notifies of a closed outgoing connection.
    ///
    /// Panics if :
    /// - too many out connections closed
    /// - the peer is unknown
    /// - too many out connections closed for that peer
    ///
    /// If the peer is not active nor bootstrap,
    /// peers are cleaned up and a dump is requested
    pub fn out_connection_closed(&mut self, ip: &IpAddr) {
        if self.active_out_connections == 0 {
            panic!("too many out connections closed");
        }
        let peer = self
            .peers
            .get_mut(&ip)
            .expect("unknown peer out connection closed");
        if peer.active_out_connections == 0 {
            panic!("too many out connections closed for peer");
        }
        self.active_out_connections -= 1;
        peer.active_out_connections -= 1;
        if !peer.is_active() && !peer.bootstrap {
            cleanup_peers(&self.cfg, &mut self.peers, None);
            self.request_dump();
        }
    }

    /// Notifies that an inbound connection is closed.
    ///
    /// Panics if :
    /// - too many in connections closed
    /// - the peer is unknown
    /// - too many in connections closed for that peer
    ///
    /// If the peer is not active nor bootstrap
    /// peers are cleaned up and a dump is requested.
    pub fn in_connection_closed(&mut self, ip: &IpAddr) {
        if self.active_in_connections == 0 {
            panic!("too many in connections closed");
        }
        let peer = self
            .peers
            .get_mut(&ip)
            .expect("unknown peer in connection closed");
        if peer.active_in_connections == 0 {
            panic!("too many in connections closed for peer");
        }
        self.active_in_connections -= 1;
        peer.active_in_connections -= 1;
        if !peer.is_active() && !peer.bootstrap {
            cleanup_peers(&self.cfg, &mut self.peers, None);
            self.request_dump();
        }
    }

    /// Yay an out connection attempt succeded.
    /// returns false if there are no slots left for out connections.
    /// The peer is set to advertized.
    ///
    /// Panics if :
    /// - too many out connection attempts succeeded
    /// - an unknown peer connection attempt succeeded
    /// - too many out connection attempts succeded for that peer
    ///
    /// A dump is requested.
    pub fn try_out_connection_attempt_success(&mut self, ip: &IpAddr) -> bool {
        // a connection attempt succeeded
        // remove out connection attempt and add out connection
        if self.active_out_connection_attempts == 0 {
            panic!("too many out connection attempts succeeded");
        }
        if self.active_out_connections >= self.cfg.target_out_connections {
            return false;
        }
        let peer = self
            .peers
            .get_mut(&ip)
            .expect("unknown peer connection attempt succeeded");
        if peer.active_out_connection_attempts == 0 {
            panic!("too many out connection attempts succeeded for peer");
        }
        self.active_out_connection_attempts -= 1;
        peer.active_out_connection_attempts -= 1;
        peer.advertised = true; // we just connected to it. Assume advertised.
        if peer.banned {
            peer.last_failure = Some(Utc::now());
            if !peer.is_active() && !peer.bootstrap {
                cleanup_peers(&self.cfg, &mut self.peers, None);
            }
            self.request_dump();
            return false;
        }
        self.active_out_connections += 1;
        peer.active_out_connections += 1;
        self.request_dump();
        true
    }

    /// Oh no an out connection attempt failed.
    ///
    /// Panics if:
    /// - too many out connection attempts failed
    /// - an unknown peer connection attempt failed
    /// - too many out connection attampts failed for tha peer
    ///
    /// A dump is requested.
    pub fn out_connection_attempt_failed(&mut self, ip: &IpAddr) {
        if self.active_out_connection_attempts == 0 {
            panic!("too many out connection attempts failed");
        }
        let peer = self
            .peers
            .get_mut(&ip)
            .expect("unknown peer connection attempt failed");
        if peer.active_out_connection_attempts == 0 {
            panic!("too many out connection attempts failures for peer");
        }
        self.active_out_connection_attempts -= 1;
        peer.active_out_connection_attempts -= 1;
        peer.last_failure = Some(Utc::now());
        if !peer.is_active() && !peer.bootstrap {
            cleanup_peers(&self.cfg, &mut self.peers, None);
        }
        self.request_dump();
    }

    /// An ip has successfully connected to us.
    /// returns true if some in slots for connections are left.
    /// If the corresponding peer exists, it is updated,
    /// otherwise it is created (not advertised).
    /// A dump is requested.
    pub fn try_new_in_connection(&mut self, ip: &IpAddr) -> bool {
        // try to create a new input connection, return false if no slots
        if !ip.is_global()
            || self.active_in_connections >= self.cfg.max_in_connections
            || self.cfg.max_in_connections_per_ip == 0
        {
            return false;
        }
        if let Some(our_ip) = self.cfg.routable_ip {
            // avoid our own IP
            if *ip == our_ip {
                return false;
            }
        }
        let peer = self.peers.entry(*ip).or_insert(PeerInfo {
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
        if peer.banned {
            peer.last_failure = Some(Utc::now());
            self.request_dump();
            return false;
        }
        if peer.active_in_connections >= self.cfg.max_in_connections_per_ip {
            self.request_dump();
            return false;
        }
        self.active_in_connections += 1;
        peer.active_in_connections += 1;
        self.request_dump();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::config::NetworkConfig;
    use super::*;

    fn example_network_config() -> NetworkConfig {
        use std::net::{IpAddr, Ipv4Addr, SocketAddr};

        NetworkConfig {
            bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080),
            routable_ip: Some(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))),
            protocol_port: 0,
            connect_timeout: std::time::Duration::from_secs_f32(180.0),
            wakeup_interval: std::time::Duration::from_secs_f32(10.0),
            peers_file: std::path::PathBuf::new(),
            target_out_connections: 10,
            max_in_connections: 5,
            max_in_connections_per_ip: 2,
            max_out_connnection_attempts: 15,
            max_idle_peers: 3,
            max_banned_peers: 3,
            max_advertise_length: 5,
            peers_file_dump_interval: std::time::Duration::from_secs_f32(30.0),
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
                    _ => Some(
                        Utc::now()
                            - chrono::Duration::from_std(std::time::Duration::from_secs(
                                rng.gen_range(0, 10000),
                            ))
                            .unwrap(),
                    ),
                },
                last_failure: match i % 5 {
                    0 => None,
                    _ => Some(
                        Utc::now()
                            - chrono::Duration::from_std(std::time::Duration::from_secs(
                                rng.gen_range(0, 10000),
                            ))
                            .unwrap(),
                    ),
                },
                advertised: (ip[2] % 2) == 0,
                active_out_connection_attempts: 0,
                active_out_connections: 0,
                active_in_connections: 0,
            };
            peers.insert(peer.ip, peer);
        }
        let cfg = example_network_config();
        let peers_file = cfg.peers_file.clone();
        let peers_file_dump_interval = cfg.peers_file_dump_interval;

        let (saver_watch_tx, mut saver_watch_rx) = watch::channel(peers.clone());
        let saver_join_handle = tokio::spawn(async move {});
        PeerInfoDatabase {
            cfg,
            peers,
            saver_join_handle,
            saver_watch_tx,
            active_out_connection_attempts: 0,
            active_out_connections: 0,
            active_in_connections: 0,
        }
    }

    #[tokio::test]
    async fn test() {
        let peer_db = peer_database_example(5);
        let p = peer_db.peers.values().next().unwrap();
        assert_eq!(p.is_active(), false);
    }
}
