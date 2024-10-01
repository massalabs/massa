use massa_channel::sender::MassaSender;
use massa_protocol_exports::{BootstrapPeers, PeerId};
use massa_time::MassaTime;
use parking_lot::RwLock;
use peernet::transports::TransportType;
use rand::seq::SliceRandom;
use rand::{thread_rng, Rng};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::time::Duration;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tracing::info;

use crate::wrap_peer_db::PeerDBTrait;

use super::announcement::Announcement;

const THREE_DAYS_MS: u64 = 3 * 24 * 60 * 60 * 1_000;

pub type InitialPeers = HashMap<PeerId, HashMap<SocketAddr, TransportType>>;

#[derive(Clone, Eq, PartialEq)]
pub struct ConnectionMetadata {
    pub last_success: Option<MassaTime>,
    pub last_failure: Option<MassaTime>,
    pub last_try_connect: Option<MassaTime>,
    pub last_test_success: Option<MassaTime>,
    pub last_test_failure: Option<MassaTime>,
    random_priority: u64,
}

impl Default for ConnectionMetadata {
    fn default() -> Self {
        ConnectionMetadata {
            last_test_success: Default::default(),
            last_test_failure: Default::default(),
            last_success: Default::default(),
            last_failure: Default::default(),
            last_try_connect: Default::default(),
            random_priority: thread_rng().gen(),
        }
    }
}

impl Ord for ConnectionMetadata {
    fn cmp(&self, other: &Self) -> Ordering {
        // Time since last failure, more recent = less priority
        let failure_check = match (self.last_failure, other.last_failure) {
            (Some(sf), Some(of)) => Some(sf.cmp(&of)),
            (Some(_), None) => Some(Ordering::Greater),
            (None, Some(_)) => Some(Ordering::Less),
            (None, None) => None,
        };
        if let Some(res) = failure_check {
            return res;
        }

        // Time since last success, more recent = more priority
        let success_check = match (self.last_success, other.last_success) {
            (Some(ss), Some(os)) => Some(ss.cmp(&os).reverse()),
            (Some(_), None) => Some(Ordering::Less),
            (None, Some(_)) => Some(Ordering::Greater),
            (None, None) => None,
        };
        if let Some(res) = success_check {
            return res;
        }

        // Time since last failed peer test, more recent = less priority
        let test_failure_check = match (self.last_test_failure, other.last_test_failure) {
            (Some(st), Some(other_)) => Some(st.cmp(&other_)),
            (Some(_), None) => Some(Ordering::Greater),
            (None, Some(_)) => Some(Ordering::Less),
            (None, None) => None,
        };
        if let Some(res) = test_failure_check {
            return res;
        }

        // Time since last succeeded peer test, more recent = more priority
        let test_success_check = match (self.last_test_success, other.last_test_success) {
            (Some(st), Some(other_)) => Some(st.cmp(&other_).reverse()),
            (Some(_), None) => Some(Ordering::Less),
            (None, Some(_)) => Some(Ordering::Greater),
            (None, None) => None,
        };
        if let Some(res) = test_success_check {
            res
        // Else, pick randomly
        } else {
            self.random_priority.cmp(&other.random_priority)
        }
    }
}

// Priorisation of a peer compared to another one
// Greater = Less Prio        Lesser = More prio
impl PartialOrd for ConnectionMetadata {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl ConnectionMetadata {
    // Only used in tests
    #[allow(dead_code)]
    pub fn edit(self, data_type: usize, data: Option<MassaTime>) -> ConnectionMetadata {
        match data_type {
            0 => ConnectionMetadata {
                last_failure: data,
                ..self
            },
            1 => ConnectionMetadata {
                last_success: data,
                ..self
            },
            2 => ConnectionMetadata {
                last_test_failure: data,
                ..self
            },
            3 => ConnectionMetadata {
                last_test_success: data,
                ..self
            },
            _ => unreachable!("connection metadata data_type not recognized: {data_type}"),
        }
    }
    pub fn failure(&mut self) {
        self.last_failure = Some(MassaTime::now());
    }

    pub fn test_failure(&mut self) {
        self.last_test_failure = Some(MassaTime::now());
    }

    pub fn test_success(&mut self) {
        self.last_test_success = Some(MassaTime::now());
    }

    pub fn success(&mut self) {
        self.last_success = Some(MassaTime::now());
    }

    pub fn try_connect(&mut self) {
        self.last_try_connect = Some(MassaTime::now());
    }
}

#[derive(Default, Clone)]
pub struct PeerDB {
    pub peers: HashMap<PeerId, PeerInfo>,
    /// Tested addresses used to avoid testing the same address too often. //TODO: Need to be pruned
    pub tested_addresses: HashMap<SocketAddr, MassaTime>,
    /// history of try connection to peers
    pub try_connect_history: HashMap<SocketAddr, ConnectionMetadata>,
    /// peers currently tested
    pub peers_in_test: HashSet<SocketAddr>,
}

pub type SharedPeerDB = Arc<RwLock<dyn PeerDBTrait>>;

pub type PeerMessageTuple = (PeerId, Vec<u8>);

#[derive(Clone, Debug)]
pub struct PeerInfo {
    pub last_announce: Option<Announcement>,
    pub state: PeerState,
}

#[warn(dead_code)]
#[derive(Eq, PartialEq, Clone, Debug)]
pub enum PeerState {
    Banned,
    InHandshake,
    HandshakeFailed,
    Trusted,
}

#[derive(Clone)]
pub enum PeerManagementCmd {
    Ban(Vec<PeerId>),
    Unban(Vec<PeerId>),
    GetBootstrapPeers {
        responder: MassaSender<BootstrapPeers>,
    },
    Stop,
}

#[allow(dead_code)]
pub struct PeerManagementChannel {
    pub msg_sender: MassaSender<PeerMessageTuple>,
    pub command_sender: MassaSender<PeerManagementCmd>,
}

impl PeerDBTrait for PeerDB {
    fn ban_peer(&mut self, peer_id: &PeerId) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.state = PeerState::Banned;
            info!("Banned peer: {:?}", peer_id);
        } else {
            info!("Tried to ban unknown peer: {:?}", peer_id);
        };
    }

    fn unban_peer(&mut self, peer_id: &PeerId) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            // We set the state to HandshakeFailed to force the peer to be tested again
            peer.state = PeerState::HandshakeFailed;
            info!("Unbanned peer: {:?}", peer_id);
        } else {
            info!("Tried to unban unknown peer: {:?}", peer_id);
        };
    }

    /// Retrieve the peer with the oldest test date.
    fn get_oldest_peer(
        &self,
        cooldown: Duration,
        in_test: &HashSet<SocketAddr>,
    ) -> Option<SocketAddr> {
        match self
            .tested_addresses
            .iter()
            .min_by_key(|(_, timestamp)| *(*timestamp))
        {
            Some((addr, timestamp)) => {
                if !in_test.contains(addr) {
                    if timestamp.estimate_instant().ok()?.elapsed() > cooldown {
                        Some(*addr)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            None => None,
        }
    }

    /// Select max 100 peers to send to another peer
    /// The selected peers should has been online within the last 3 days
    fn get_rand_peers_to_send(
        &self,
        nb_peers: usize,
    ) -> Vec<(PeerId, HashMap<SocketAddr, TransportType>)> {
        //TODO: Add ourself
        let now = MassaTime::now().as_millis();

        let min_time = now - THREE_DAYS_MS;

        let mut keys = self.peers.keys().cloned().collect::<Vec<_>>();
        let mut rng = rand::thread_rng();
        keys.shuffle(&mut rng);

        let mut result = Vec::new();

        for key in keys {
            if result.len() >= nb_peers {
                break;
            }
            if let Some(peer) = self.peers.get(&key) {
                // skip old peers
                if let Some(last_announce) = &peer.last_announce {
                    if last_announce.timestamp < min_time {
                        continue;
                    }
                    let listeners: HashMap<SocketAddr, TransportType> =
                        last_announce.listeners.clone().into_iter().collect();
                    if listeners.is_empty() {
                        continue;
                    }
                    result.push((key, listeners));
                }
            }
        }

        result
    }

    fn clone_box(&self) -> Box<dyn PeerDBTrait> {
        Box::new(self.clone())
    }

    fn get_banned_peer_count(&self) -> u64 {
        self.peers
            .values()
            .filter(|peer| peer.state == PeerState::Banned)
            .count() as u64
    }

    fn get_known_peer_count(&self) -> u64 {
        self.peers.len() as u64
    }

    fn get_peers(&self) -> &HashMap<PeerId, PeerInfo> {
        &self.peers
    }

    fn get_peers_mut(&mut self) -> &mut HashMap<PeerId, PeerInfo> {
        &mut self.peers
    }

    fn get_connection_metadata_or_default(&self, addr: &SocketAddr) -> ConnectionMetadata {
        self.try_connect_history
            .get(addr)
            .cloned()
            .unwrap_or(ConnectionMetadata::default())
    }

    fn set_try_connect_success_or_insert(&mut self, addr: &SocketAddr) {
        self.try_connect_history
            .entry(*addr)
            .or_default()
            .try_connect();
    }

    fn set_try_connect_failure_or_insert(&mut self, addr: &SocketAddr) {
        self.try_connect_history.entry(*addr).or_default().failure();
    }

    fn set_try_connect_test_success_or_insert(&mut self, addr: &SocketAddr) {
        self.try_connect_history
            .entry(*addr)
            .or_default()
            .test_success();
    }

    fn set_try_connect_test_failure_or_insert(&mut self, addr: &SocketAddr) {
        self.try_connect_history
            .entry(*addr)
            .or_default()
            .test_failure();
    }

    fn get_peers_in_test(&self) -> &HashSet<SocketAddr> {
        &self.peers_in_test
    }

    fn insert_peer_in_test(&mut self, addr: &SocketAddr) -> bool {
        self.peers_in_test.insert(*addr)
    }

    fn remove_peer_in_test(&mut self, addr: &SocketAddr) -> bool {
        self.peers_in_test.remove(addr)
    }

    fn insert_tested_address(&mut self, addr: &SocketAddr, time: MassaTime) {
        self.tested_addresses.insert(*addr, time);
    }

    fn get_tested_addresses(&self) -> &HashMap<SocketAddr, MassaTime> {
        &self.tested_addresses
    }
}
