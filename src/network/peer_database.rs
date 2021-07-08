use chrono::{DateTime, Utc};
use log::warn;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::net::IpAddr;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{delay_for, Duration};

type BoxResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(PartialEq, Eq, Clone, Copy, Serialize, Deserialize, Debug)]
pub enum PeerStatus {
    Idle,
    InHandshaking,
    InAlive,
    OutConnecting,
    OutHandshaking,
    OutAlive,
    Banned,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
pub struct PeerInfo {
    pub ip: IpAddr,
    pub status: PeerStatus,
    pub advertised_as_reachable: bool,
    pub bootstrap: bool,
    pub last_alive: Option<DateTime<Utc>>,
    pub last_failure: Option<DateTime<Utc>>,
}

pub struct PeerDatabase {
    pub peers: HashMap<IpAddr, PeerInfo>,
    saver_join_handle: JoinHandle<()>,
    saver_watch_tx: watch::Sender<Option<HashMap<IpAddr, PeerInfo>>>,
}

async fn load_peers(file_name: &String) -> BoxResult<HashMap<IpAddr, PeerInfo>> {
    let result =
        serde_json::from_str::<Vec<PeerInfo>>(&tokio::fs::read_to_string(file_name).await?)?
            .iter()
            .filter(|&p| p.advertised_as_reachable || p.bootstrap)
            .map(|&p| {
                (
                    p.ip,
                    PeerInfo {
                        status: match p.status {
                            PeerStatus::Banned => PeerStatus::Banned,
                            _ => PeerStatus::Idle,
                        },
                        ..p
                    },
                )
            })
            .collect::<HashMap<IpAddr, PeerInfo>>();
    if result.is_empty() {
        return Err("known peers file is empty".into());
    }
    Ok(result)
}

async fn dump_peers(peers: &HashMap<IpAddr, PeerInfo>, file_name: &String) -> BoxResult<()> {
    let peer_vec: Vec<PeerInfo> = peers
        .values()
        .filter(|&p| p.advertised_as_reachable || p.bootstrap)
        .map(|&p| PeerInfo {
            status: match p.status {
                PeerStatus::Banned => PeerStatus::Banned,
                _ => PeerStatus::Idle,
            },
            ..p
        })
        .collect();
    tokio::fs::write(file_name, serde_json::to_string_pretty(&peer_vec)?).await?;
    Ok(())
}

impl PeerDatabase {
    pub async fn load(
        peers_filename: String,
        peer_file_dump_interval_seconds: f32,
    ) -> BoxResult<Self> {
        // load from file
        let peers = load_peers(&peers_filename).await?;

        // setup saver
        let peers_filename_copy = peers_filename.clone();
        let (saver_watch_tx, mut saver_watch_rx) = watch::channel(Some(peers.clone()));
        let saver_join_handle = tokio::spawn(async move {
            let mut delay = delay_for(Duration::from_secs_f32(peer_file_dump_interval_seconds));
            let mut last_value: Option<HashMap<IpAddr, PeerInfo>> = None;
            loop {
                tokio::select! {
                    opt_opt_p = saver_watch_rx.recv() => match opt_opt_p {
                        Some(Some(op)) => {
                            if last_value.is_none() {
                                delay = delay_for(Duration::from_secs_f32(peer_file_dump_interval_seconds));
                            }
                            last_value = Some(op);
                        },
                        _ => break
                    },
                    _ = &mut delay => {
                        if let Some(ref p) = last_value {
                            if let Err(e) = dump_peers(&p, &peers_filename_copy).await {
                                warn!("could not dump peers to file: {}", e);
                                delay = delay_for(Duration::from_secs_f32(peer_file_dump_interval_seconds));
                                continue;
                            }
                            last_value = None;
                        }
                    }
                }
            }
            if let Some(p) = last_value {
                if let Err(e) = dump_peers(&p, &peers_filename_copy).await {
                    warn!("could not dump peers to file: {}", e);
                }
            }
        });

        // return struct
        Ok(PeerDatabase {
            peers,
            saver_join_handle,
            saver_watch_tx,
        })
    }

    pub fn save(&self) {
        if self
            .saver_watch_tx
            .broadcast(Some(self.peers.clone()))
            .is_err()
        {
            unreachable!("saver task disappeared");
        }
    }

    pub async fn stop(self) {
        let _ = self.saver_watch_tx.broadcast(None);
        let _ = self.saver_join_handle.await;
    }

    pub fn count_peers_with_status(&self, status: PeerStatus) -> usize {
        self.peers.values().filter(|&v| v.status == status).count()
    }

    pub fn cleanup(&mut self, max_idle_peers: usize, max_banned_peers: usize) {
        // remove any idle non-reachable non-bootstrap peers
        self.peers.retain(
            |&k, &mut v| match (v.status, v.advertised_as_reachable, v.bootstrap) {
                (PeerStatus::Idle, false, false) => false,
                _ => true,
            },
        );

        // remove excess idle non-bootstrap peers
        let n_idle_nonbootstrap_peers = self
            .peers
            .values()
            .filter(|&p| (p.status, p.bootstrap) == (PeerStatus::Idle, false))
            .count();
        if n_idle_nonbootstrap_peers > max_idle_peers {
            let mut keep_ips: Vec<IpAddr> = self
                .peers
                .values()
                .filter(|&p| (p.status, p.bootstrap) == (PeerStatus::Idle, false))
                .map(|&p| p.ip)
                .collect();
            keep_ips.sort_unstable_by_key(|&k| {
                let p = self.peers.get(&k).unwrap(); // should never fail
                (std::cmp::Reverse(p.last_alive), p.last_failure)
            });
            keep_ips.truncate(max_idle_peers);
            self.peers
                .retain(|&k, &mut v| match (v.status, v.bootstrap) {
                    (PeerStatus::Idle, false) => keep_ips.contains(&k),
                    _ => true,
                })
        }

        // remove excess banned peers
        let n_banned_nonbootstrap_peers = self
            .peers
            .values()
            .filter(|&p| (p.status, p.bootstrap) == (PeerStatus::Banned, false))
            .count();
        if n_banned_nonbootstrap_peers > max_banned_peers {
            let mut keep_ips: Vec<IpAddr> = self
                .peers
                .values()
                .filter(|&p| (p.status, p.bootstrap) == (PeerStatus::Banned, false))
                .map(|&p| p.ip)
                .collect();
            keep_ips.sort_unstable_by_key(|&k| {
                let p = self.peers.get(&k).unwrap(); // should never fail
                (std::cmp::Reverse(p.last_failure), p.last_alive);
            });
            keep_ips.truncate(max_banned_peers);
            self.peers
                .retain(|&k, &mut v| match (v.status, v.bootstrap) {
                    (PeerStatus::Banned, false) => keep_ips.contains(&k),
                    _ => true,
                })
        }
    }

    pub fn get_connector_candidate_ips(
        &self,
        target_outgoing_connections: usize,
        max_simultaneous_outgoing_connection_attempts: usize,
    ) -> HashSet<IpAddr> {
        let n_available_attempts = std::cmp::min(
            target_outgoing_connections
                .saturating_sub(self.count_peers_with_status(PeerStatus::OutAlive)),
            max_simultaneous_outgoing_connection_attempts,
        )
        .saturating_sub(
            self.count_peers_with_status(PeerStatus::OutConnecting)
                + self.count_peers_with_status(PeerStatus::OutHandshaking),
        );

        if n_available_attempts == 0 {
            return HashSet::new(); // no connections needed or possible
        }
        /*
            sort peers:
                creterion 1 = the most recent successful connection (None = oldest)
                criterion 2 = the oldest failure (None = oldest)
            then pick the n_available_attempts first options
            here we use the sequential nature of tuples and the fact that Some<T> > None
            note: using unstable sorting because stability is not ensured by hashmap anyways
        */
        let mut peers_sorted: Vec<IpAddr> = self
            .peers
            .values()
            .filter(|&p| p.status == PeerStatus::Idle)
            .map(|&p| p.ip)
            .collect();
        peers_sorted.sort_unstable_by_key(|&ip| {
            let p = self.peers.get(&ip).unwrap();
            (std::cmp::Reverse(p.last_alive), p.last_failure)
        });
        peers_sorted
            .into_iter()
            .take(n_available_attempts)
            .collect()
    }

    pub fn merge_candidate_peers(&mut self, new_peers: &HashSet<IpAddr>) {
        for &new_peer_ip in new_peers {
            self.peers
                .entry(new_peer_ip)
                .or_insert(PeerInfo {
                    ip: new_peer_ip.clone(),
                    status: PeerStatus::Idle,
                    advertised_as_reachable: true,
                    bootstrap: false,
                    last_alive: None,
                    last_failure: None,
                })
                .advertised_as_reachable = true;
        }
    }

    pub fn get_advertisable_peer_ips(&self) -> Vec<IpAddr> {
        // get the list of peers sorted from best to worst
        let mut peers_sorted: Vec<IpAddr> = self
            .peers
            .values()
            .filter(|&p| p.status != PeerStatus::Banned && p.advertised_as_reachable)
            .map(|&p| p.ip)
            .collect();
        peers_sorted.sort_unstable_by_key(|&ip| {
            let p = self.peers.get(&ip).unwrap();
            (
                std::cmp::Reverse(p.bootstrap),
                std::cmp::Reverse(p.last_alive),
                p.last_failure,
            )
        });
        peers_sorted
    }
}
