use chrono::Local;
use chrono::TimeZone;
use communication::PeerInfo;
use consensus::DiscardReason;
use crypto::hash::Hash;
use crypto::signature::Signature;
use models::{Block, BlockHeader, Operation, Slot};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::time::Duration;
use std::{collections::HashSet, net::IpAddr};
use std::{
    fmt::Display,
    sync::atomic::{AtomicBool, Ordering},
};
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
impl Display for WrappedHash {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if FORMAT_SHORT_HASH.load(Ordering::Relaxed) {
            write!(f, "{}", &self.0.to_bs58_check()[..4])
        } else {
            write!(f, "{}", &self.0.to_bs58_check())
        }
    }
}

impl Debug for WrappedHash {
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

impl Display for WrappedSlot {
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

#[derive(Debug, Clone, Deserialize)]
pub struct WrappedBlockHeader(BlockHeader);

impl From<BlockHeader> for WrappedBlockHeader {
    fn from(header: BlockHeader) -> Self {
        WrappedBlockHeader(header)
    }
}
impl From<&'_ BlockHeader> for WrappedBlockHeader {
    fn from(header: &BlockHeader) -> Self {
        WrappedBlockHeader(header.clone())
    }
}

impl Display for WrappedBlockHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let pk = self.0.content.creator.to_string();
        let pk = if FORMAT_SHORT_HASH.load(Ordering::Relaxed) {
            &pk[..4]
        } else {
            &pk
        };
        writeln!(
            f,
            "creator: {} period:{} thread:{} ledger:{} merkle_root:{} parents:{:?}",
            pk,
            self.0.content.slot.period,
            self.0.content.slot.thread,
            WrappedHash::from(self.0.content.out_ledger_hash),
            WrappedHash::from(self.0.content.operation_merkle_root),
            &self
                .0
                .content
                .parents
                .iter()
                .map(|hash| WrappedHash::from(hash).to_string())
                .collect::<Vec<String>>(),
        )
        //        writeln!(f, "  endorsements:{:?}", self.endorsements)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct WrapperBlock {
    pub header: WrappedBlockHeader,
    pub operations: Vec<Operation>,
    pub signature: Signature,
}

impl From<Block> for WrapperBlock {
    fn from(block: Block) -> Self {
        WrapperBlock {
            operations: block.operations,
            signature: block.header.signature,
            header: block.header.into(),
        }
    }
}

impl Display for WrapperBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let signature = self.signature.to_string();
        let signature = if FORMAT_SHORT_HASH.load(Ordering::Relaxed) {
            &signature[..4]
        } else {
            &signature
        };
        write!(f, "{} signature:{} operations:....", self.header, signature)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StakerInfo {
    pub staker_active_blocks: Vec<(Hash, BlockHeader)>,
    pub staker_discarded_blocks: Vec<(Hash, DiscardReason, BlockHeader)>,
    pub staker_next_draws: Vec<Slot>,
}

impl Display for StakerInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "  active blocks:")?;
        let mut blocks: Vec<&(Hash, BlockHeader)> = self.staker_active_blocks.iter().collect();
        blocks.sort_unstable_by_key(|v| (v.1.content.slot, v.0));
        for (hash, block) in &blocks {
            write!(
                f,
                "    block: hash:{} header: {}",
                WrappedHash::from(hash),
                WrappedBlockHeader::from(block)
            )?;
        }
        writeln!(f, "  discarded blocks:")?;
        let mut blocks: Vec<&(Hash, DiscardReason, BlockHeader)> =
            self.staker_discarded_blocks.iter().collect();
        blocks.sort_unstable_by_key(|v| (v.2.content.slot, v.0));
        for (hash, reason, block) in &blocks {
            write!(
                f,
                "    block: hash:{} reason:{:?} header: {}",
                WrappedHash::from(hash),
                reason,
                WrappedBlockHeader::from(block)
            )?;
        }
        writeln!(
            f,
            "  staker_next_draws{:?}:",
            self.staker_next_draws
                .iter()
                .map(|slot| format!("(slot:{})", slot))
                .collect::<Vec<String>>()
        )
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Clone)]
pub struct HashSlot {
    pub hash: Hash,
    pub slot: Slot,
}

impl Display for HashSlot {
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
    list.sort_unstable_by_key(|v| (v.slot, v.hash));
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

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct BlockInfo {
    pub hash_slot: HashSlot,
    pub status: StatusInfo,
    pub parents: Vec<Hash>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum StatusInfo {
    Stale,
    Active,
    Final,
}

impl Display for StatusInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StatusInfo::Stale => write!(f, "stale"),
            StatusInfo::Active => write!(f, "active"),
            StatusInfo::Final => write!(f, "final"),
        }
    }
}

// impl From<(Hash, Slot, String, Vec<Hash>)> for BlockInfo {
//     fn from((hash, slot, status, parents): (Hash, Slot, String, Vec<Hash>)) -> Self {
//         BlockInfo{ hash_slot: (hash, slot).into(), status, parents}
//     }
// }

impl Display for BlockInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "Block: {} Status:{}", self.hash_slot, self.status)?;
        writeln!(
            f,
            "Block parents: {:?}",
            self.parents
                .iter()
                .map(|h| h.into())
                .collect::<Vec<WrappedHash>>()
        )
        // writeln!(f)
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

impl Display for Cliques {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "Nb of cliques: {}", self.number)?;
        writeln!(f, "Cliques: ")?;
        self.content
            .iter()
            .map(|clique| {
                let mut list: Vec<&HashSlot> = clique.iter().collect();
                list.sort_unstable_by_key(|v| (v.slot, v.hash));
                let formated = format_hash_slot_list(&list);
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

impl Display for NetworkInfo {
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
pub struct WrappedPeerInfo(PeerInfo);

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
pub struct HashSlotTime {
    pub hash_slot: HashSlot,
    pub time: UTime,
}

impl Display for HashSlotTime {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{} {:?}",
            self.hash_slot,
            Local.timestamp(Into::<Duration>::into(self.time).as_secs() as i64, 0)
        )
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct State {
    pub time: UTime,
    pub latest_slot: Option<Slot>,
    pub our_ip: Option<IpAddr>,
    pub last_final: Vec<HashSlotTime>,
    pub nb_cliques: u64,
    pub nb_peers: u64,
}
impl Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let duration: Duration = self.time.into();

        let date = Local.timestamp(duration.as_secs() as i64, 0);
        write!(
            f,
            "  Time: {:?} Latest:{}",
            date,
            self.latest_slot
                .map(|s| format!("Slot {:?}", s))
                .unwrap_or("None".to_string())
        )?;
        write!(
            f,
            " Nb peers: {}, our IP: {}",
            self.nb_peers,
            self.our_ip
                .map(|i| i.to_string())
                .unwrap_or("None".to_string())
        )?;
        let mut final_blocks: Vec<&HashSlotTime> = self.last_final.iter().collect();
        final_blocks.sort_unstable_by_key(|v| (v.hash_slot.slot, v.hash_slot.hash));

        write!(
            f,
            " Nb cliques: {}, last final blocks:{:#?}",
            self.nb_cliques,
            final_blocks
                .iter()
                .map(|hash_slot| hash_slot.to_string())
                .collect::<Vec<String>>()
        )
    }
}
