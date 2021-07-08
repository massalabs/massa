use std::{
    collections::{HashMap, HashSet},
    net::IpAddr,
};

use communication::PeerInfo;
use consensus::DiscardReason;
use crypto::hash::Hash;
use models::{BlockHeader, Slot};
use serde::{Deserialize, Serialize};
use time::UTime;

#[derive(Debug, Serialize, Deserialize)]
pub struct StakerInfo {
    pub staker_active_blocks: Vec<(Hash, BlockHeader)>,
    pub staker_discarded_blocks: Vec<(Hash, DiscardReason, BlockHeader)>,
    pub staker_next_draws: Vec<Slot>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
struct HashSlot {
    hash: Hash,
    slot: Slot,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Parents {
    content: Vec<HashSlot>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Invalids {
    content: Vec<HashSlot>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Finals {
    content: Vec<HashSlot>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Stales {
    content: Vec<HashSlot>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlockInterval {
    content: Vec<HashSlot>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BlockInfo {
    hash: Hash,
    slot: Slot,
    status: String,
    parents: Vec<Hash>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GraphInterval {
    content: Vec<BlockInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Cliques {
    number: u64,
    content: Vec<HashSet<HashSlot>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkInfo {
    our_ip: Option<IpAddr>,
    peers: HashMap<IpAddr, PeerInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Peers {
    content: HashMap<IpAddr, PeerInfo>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
struct HashSlotTime {
    hash: Hash,
    slot: Slot,
    time: UTime,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct State {
    time: UTime,
    latest_slot: Option<Slot>,
    our_ip: Option<IpAddr>,
    last_final: Vec<HashSlotTime>,
    nb_cliques: u64,
    nb_peers: u64,
}
