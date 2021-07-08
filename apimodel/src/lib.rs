use chrono::Local;
use chrono::TimeZone;
use communication::PeerInfo;
use consensus::DiscardReason;
use crypto::hash::Hash;
use models::{BlockHeader, Slot};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::{collections::HashSet, net::IpAddr};
use time::UTime;

pub static FORMAT_SHORT_HASH: AtomicBool = AtomicBool::new(true);

#[derive(Eq, PartialEq, Ord, PartialOrd, Copy, Clone, Hash, Deserialize)]
pub struct WrappedHash(Hash);

impl From<Hash> for WrappedHash {
    fn from(hash: Hash) -> Self {
        WrappedHash(hash)
    }
}
impl From<&'_ Hash> for WrappedHash {
    fn from(hash: &Hash) -> Self {
        WrappedHash(*hash)
    }
}
impl std::fmt::Display for WrappedHash {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if FORMAT_SHORT_HASH.load(Ordering::Relaxed) {
            write!(f, "{}", &self.0.to_bs58_check()[..4])
        } else {
            write!(f, "{}", &self.0.to_bs58_check())
        }
    }
}

impl std::fmt::Debug for WrappedHash {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if FORMAT_SHORT_HASH.load(Ordering::Relaxed) {
            write!(f, "{}", &self.0.to_bs58_check()[..4])
        } else {
            write!(f, "{}", &self.0.to_bs58_check())
        }
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub struct WrappedSlot(Slot);

impl std::fmt::Display for WrappedSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "period:{} thread:{}", self.0.period, self.0.thread)
    }
}

impl From<Slot> for WrappedSlot {
    fn from(slot: Slot) -> Self {
        WrappedSlot(slot)
    }
}
impl From<&'_ Slot> for WrappedSlot {
    fn from(slot: &Slot) -> Self {
        WrappedSlot(*slot)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StakerInfo {
    pub staker_active_blocks: Vec<(Hash, BlockHeader)>,
    pub staker_discarded_blocks: Vec<(Hash, DiscardReason, BlockHeader)>,
    pub staker_next_draws: Vec<Slot>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct HashSlot {
    pub hash: Hash,
    pub slot: Slot,
}

impl From<(Hash,Slot)> for HashSlot {
    fn from((hash, slot) :(Hash,Slot)) -> Self {
        HashSlot { hash, slot }
    }
}
impl std::fmt::Display for HashSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{} slot:{}",
            WrappedHash::from(self.hash),
            WrappedSlot::from(self.slot)
        )
    }
}

impl From<(Hash, Slot)> for HashSlot {
    fn from((hash, slot): (Hash, Slot)) -> Self {
        HashSlot { hash, slot }
    }
}

//doesn't impl From because Vec and From are ouside this crate.
pub fn from_vec_hash_slot(list: &[(Hash, Slot)]) -> Vec<HashSlot> {
    list.into_iter().map(|v| (*v).into()).collect()
}

///Construct a list of diplay String from the specified list of Hash
///The hash are sorted with their slot (periode) number
///
///The input parameter list is a collection of tuple (Hash, Slot)
/// return a list of string the display.
fn format_hash_slot_list(hash_slots: &[&HashSlot]) -> Vec<String> {
    let mut list: Vec<&&HashSlot> = hash_slots.iter().collect();
    list.sort_unstable_by(|a, b| a.slot.cmp(&b.slot));
    list.iter()
        .map(|hash_slot| format!("({})", hash_slot))
        .collect()
}

/*
#[derive(Debug, Serialize, Deserialize)]
pub struct Parents {
    content: Vec<HashSlot>,
}
impl std::fmt::Display for Parents {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let formated = format_hash_slot_list(&self.content);
        write!(f, "Parents:{:#?}", formated)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Invalids {
    pub content: Vec<HashSlot>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Finals {
    pub content: Vec<HashSlot>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Stales {
    pub content: Vec<HashSlot>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlockInterval {
    pub content: Vec<HashSlot>,
}*/

#[derive(Debug, Serialize, Deserialize)]
pub struct BlockInfo {
    pub hash_slot: HashSlot,
    pub status: String,
    pub parents: Vec<Hash>,
}

impl std::fmt::Display for BlockInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "Block: {} Status:{}", self.hash_slot, self.status)?;
        writeln!(
            f,
            "Block parents: {:?}",
            self.parents
                .iter()
                .map(|h| h.into())
                .collect::<Vec<WrappedHash>>()
        )?;
        writeln!(f)
    }
}

/*#[derive(Debug, Serialize, Deserialize)]
pub struct GraphInterval {
    pub content: Vec<BlockInfo>,
}*/

#[derive(Debug, Serialize, Deserialize)]
pub struct Cliques {
    pub number: u64,
    pub content: Vec<HashSet<HashSlot>>,
}

impl std::fmt::Display for Cliques {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "Nb of cliques: {}", self.number)?;
        writeln!(f, "Cliques: ")?;
        self.content
            .iter()
            .map(|clique| {
                let formated = format_hash_slot_list(&clique.iter().collect::<Vec<&HashSlot>>());
                writeln!(f, "{:#?}", formated)
            })
            .collect()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkInfo {
    pub our_ip: Option<IpAddr>,
    pub peers: Vec<PeerInfo>,
}

impl std::fmt::Display for NetworkInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(
            f,
            "  Our IP address: {}",
            self.our_ip
                .map(|i| i.to_string())
                .unwrap_or("None".to_string())
        )?;
        writeln!(f, "  Peers:")?;
        self.peers
            .iter()
            .map(|peer| write!(f, "    {}", WrappedPeerInfo::from(peer)))
            .collect()
    }
}

#[derive(Clone, Deserialize)]
struct WrappedPeerInfo(PeerInfo);

impl From<PeerInfo> for WrappedPeerInfo {
    fn from(peer: PeerInfo) -> Self {
        WrappedPeerInfo(peer)
    }
}
impl From<&'_ PeerInfo> for WrappedPeerInfo {
    fn from(peer: &PeerInfo) -> Self {
        WrappedPeerInfo(peer.clone())
    }
}
impl std::fmt::Display for WrappedPeerInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "Peer: Ip:{} bootstrap:{} banned:{} last_alive:{} last_failure:{} act_out_attempts:{} act_out:{} act_in:{}"
            ,self.0.ip
            , self.0.banned
            , self.0.bootstrap
            , self.0.last_alive.map(|t| format!("{:?}",Local.timestamp(Into::<Duration>::into(t).as_secs() as i64, 0))).unwrap_or("None".to_string())
            , self.0.last_failure.map(|t| format!("{:?}",Local.timestamp(Into::<Duration>::into(t).as_secs() as i64, 0))).unwrap_or("None".to_string())
            , self.0.active_out_connection_attempts
            , self.0.active_out_connections
            , self.0.active_in_connections)
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
struct HashSlotTime {
    pub hash_slot: HashSlot,
    pub time: UTime,
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
