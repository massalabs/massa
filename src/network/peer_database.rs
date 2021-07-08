use chrono::{DateTime, Utc};
use log::warn;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::net::IpAddr;
use tokio::fs::{read_to_string, write};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{delay_for, Duration};
use serde::{Serialize, Deserialize};
use toml;

type BoxResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub enum PeerStatus {
    Idle,
    OutConnecting,
    OutHandshaking,
    OutConnected,
    InHandshaking,
    InConnected,
    Banned,
}
impl Default for PeerStatus {
    fn default() -> Self { PeerStatus::Idle }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct PeerInfo {
    pub ip: IpAddr,
    pub status: PeerStatus,
    pub last_connection: Option<DateTime<Utc>>,
    pub last_failure: Option<DateTime<Utc>>,
}

pub struct PeerDatabase {
    pub peers: HashMap<IpAddr, PeerInfo>,
    peers_filename: String,
    saver_join_handle: JoinHandle<()>,
    saver_watch_tx: watch::Sender<Option<Vec<String>>>,
}

async fn load_peers(file_name: &String) -> BoxResult<HashMap<IpAddr, PeerInfo>> {
    
    /*
        TODO load from file
        if status is not Idle or Banned, raise error
    */

    let mut result = HashMap::<IpAddr, PeerInfo>::new();

    let text = read_to_string(file_name).await?;
    for ip_str in text.lines() {
        let ip: IpAddr = ip_str.to_string().parse()?;
        result.insert(
            ip,
            PeerInfo {
                ip,
                status: PeerStatus::Idle,
                last_connection: None,
                last_failure: None,
            },
        );
    }
    if result.len() == 0 {
        return Err("known peers file is empty".into());
    }
    Ok(result)
}

async fn dump_peers(peers: &HashMap<IpAddr, PeerInfo>, file_name: &String) -> BoxResult<()> {
    // TODO SAVE EVERYTHING !
    //TODO if status is not Banned, set it to idle

    Ok(())
}

impl PeerDatabase {
    pub async fn load(peers_filename: String) -> BoxResult<Self> {
        // load from file
        let peers = load_peers(&peers_filename).await?;

        // setup saver
        let peers_filename_copy = peers_filename.clone();
        let (saver_watch_tx, mut saver_watch_rx) = watch::channel(Some(Vec::<String>::new()));
        let saver_join_handle = tokio::spawn(async move {
            while let Some(Some(p)) = saver_watch_rx.recv().await {
                if let Err(e) = dump_peers(&p, &peers_filename_copy).await {
                    warn!("could not dump peers to file: {}", e);
                }
                delay_for(Duration::from_secs(10)).await; // save at most every 10 seconds  //TODO this slows down graceful shutdown
            }
        });

        // return struct
        Ok(PeerDatabase {
            peers,
            peers_filename,
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
        let _ = dump_peers(
            &self.peers.keys().map(|ip| ip.to_string()).collect(),
            &self.peers_filename,
        )
        .await;
    }

    pub fn count_peers_with_status(&self, status: PeerStatus) -> usize {
        self.peers.values().filter(|&v| v.status == status).count()
    }

    pub fn cleanup(&mut self, max_known_nodes: usize) {
        // bookkeeping: drop old nodes etc...
        /* TODO bookeeping
            removes too old etc... if too many, drop randomly

            also remove too old banned nodes
        */
    }

    pub fn get_connector_candidate_ips(&self) -> HashSet<IpAddr> {
        /*
            TODO:
                missing_out_connections = max(0, target_out_connections - count_peers_with_status(outconnected)) warning: usize is unsigned !
                connection_slots_available = max(0, max_out_conn_attempts - count_peers_with_status(outconnecting) - count_peers_with_status(outhandshaking)) warning: usize is unsigned !
                n_to_try = min(missing_out_connections, connection_slots_available)
                => choose up to n_to_try candidates based on:
                    most_ancient failure (None = most ancient)
        */
        HashSet::new() //TODO compute
    }
}
