//!Contains all the structure receive using the REST API.
//!
//!All struct implements display to be shown on the screen.
//!
//! Copy of all structure defined in the API side. They has been copied to avoid to force some behaviour on the massa node developements like display.
//! To detect desynchronisation between the 2 API, tests has been added to validated the deserialisation of the REST API call response.
//!
//! There're only deserialized when received from the REST call.

use chrono::Local;
use chrono::TimeZone;
use communication::network::PeerInfo;
use consensus::DiscardReason;
use consensus::LedgerDataExport;
use crypto::hash::Hash;
use crypto::signature::Signature;
use models::Address;
use models::BlockId;
use models::OperationType;
use models::{Block, BlockHeader, Operation, Slot};
use serde::Deserialize;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use time::UTime;

pub static FORMAT_SHORT_HASH: AtomicBool = AtomicBool::new(true); //never set to zero.

#[derive(Debug, Clone)]
pub struct WrapperOperationType<'a>(&'a OperationType);

impl<'a> From<&'a OperationType> for WrapperOperationType<'a> {
    fn from(op: &'a OperationType) -> Self {
        WrapperOperationType(&op)
    }
}

impl<'a> std::fmt::Display for WrapperOperationType<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self.0 {
            OperationType::Transaction {
                recipient_address,
                amount,
            } => write!(
                f,
                "Transaction: recipient:{} amount:{}",
                recipient_address, amount
            ),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct WrapperOperation(Operation);

impl From<Operation> for WrapperOperation {
    fn from(op: Operation) -> Self {
        WrapperOperation(op)
    }
}

impl std::fmt::Display for WrapperOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let op_type = WrapperOperationType::from(&self.0.content.op);
        let addr = Address::from_public_key(&self.0.content.sender_public_key)
            .map_err(|_| std::fmt::Error)?;
        write!(
            f,
            "Operation: sender:{} fee:{} expire_period:{} {}",
            addr, self.0.content.fee, self.0.content.expire_period, op_type
        )
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct GetOperationContent {
    pub operation: WrapperOperation,
    pub in_pool: bool,
    pub in_blocks: Vec<(BlockId, bool)>,
}

impl std::fmt::Display for GetOperationContent {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "{} in pool:{}", self.operation, self.in_pool)?;
        writeln!(
            f,
            "block list:{}",
            self.in_blocks
                .iter()
                .map(|(id, f)| format!("({}, final:{})", id, f))
                .collect::<Vec<String>>()
                .join(" ")
        )
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct ErrorMessage {
    pub message: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ConsensusConfig {
    pub t0: UTime,
    pub thread_count: u8,
    pub genesis_timestamp: UTime,
    pub delta_f0: u64,
    pub max_block_size: u32,
    pub operation_validity_periods: u64,
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

pub fn from_hash_slot((hash, slot): (Hash, Slot)) -> (WrappedHash, WrappedSlot) {
    (hash.into(), slot.into())
}

pub fn from_vec_hash_slot(list: &[(Hash, Slot)]) -> Vec<(WrappedHash, WrappedSlot)> {
    list.into_iter().map(|v| from_hash_slot(*v)).collect()
}

#[derive(Debug, Clone)]
pub struct WrapperAddressLedgerDataExport<'a> {
    pub ledger: LedgerDataExport,
    pub search_addr: &'a Address,
}

impl<'a> WrapperAddressLedgerDataExport<'a> {
    pub fn new(search_addr: &Address, ledger: LedgerDataExport) -> WrapperAddressLedgerDataExport {
        WrapperAddressLedgerDataExport {
            ledger,
            search_addr,
        }
    }
}

impl<'a> std::fmt::Display for WrapperAddressLedgerDataExport<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let final_balance = self
            .ledger
            .final_data
            .data
            .iter()
            .flatten()
            .find(|(addr, _)| {
                if addr == &self.search_addr {
                    true
                } else {
                    false
                }
            })
            .map(|(_, data)| data.get_balance().to_string())
            .unwrap_or("Not balance found".to_string());
        writeln!(f, "Balance at final blocks:{} ", final_balance)?;
        let parent_balance = self
            .ledger
            .candidate_data
            .data
            .iter()
            .flatten()
            .find(|(addr, _)| {
                if addr == &self.search_addr {
                    true
                } else {
                    false
                }
            })
            .map(|(_, data)| data.get_balance().to_string())
            .unwrap_or("Not balance found".to_string());
        writeln!(f, "Balance at best parents:{} ", parent_balance)
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

impl std::fmt::Display for WrapperBlock {
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

impl std::fmt::Display for WrappedBlockHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let pk = self.0.content.creator.to_string();
        let pk = if FORMAT_SHORT_HASH.load(Ordering::Relaxed) {
            &pk[..4]
        } else {
            &pk
        };
        writeln!(
            f,
            "creator: {} period:{} thread:{} merkle_root:{} parents:{:?}",
            pk,
            self.0.content.slot.period,
            self.0.content.slot.thread,
            self.0.content.operation_merkle_root,
            self.0.content.parents,
        )
        //        writeln!(f, "  parents:{:?}", self.parents)?;
        //        writeln!(f, "  endorsements:{:?}", self.endorsements)
    }
}

#[derive(Clone, Deserialize)]
pub struct State {
    time: UTime,
    latest_slot: Option<Slot>,
    our_ip: Option<IpAddr>,
    last_final: Vec<(Hash, Slot, UTime)>,
    nb_cliques: usize,
    nb_peers: usize,
}
impl std::fmt::Display for State {
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
        let mut final_blocks: Vec<&(Hash, Slot, UTime)> = self.last_final.iter().collect();
        final_blocks.sort_unstable_by_key(|v| (v.1, v.0));

        write!(
            f,
            " Nb cliques: {}, last final blocks:{:#?}",
            self.nb_cliques,
            final_blocks
                .iter()
                .map(|(hash, slot, date)| format!(
                    " {} slot:{:?} {:?}",
                    hash,
                    slot,
                    Local.timestamp(Into::<Duration>::into(*date).as_secs() as i64, 0)
                ))
                .collect::<Vec<String>>()
        )
    }
}

#[derive(Clone, Deserialize)]
pub struct StakerInfo {
    staker_active_blocks: Vec<(Hash, BlockHeader)>,
    staker_discarded_blocks: Vec<(Hash, DiscardReason, BlockHeader)>,
    staker_next_draws: Vec<Slot>,
}

impl std::fmt::Display for StakerInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "  active blocks:")?;
        let mut blocks: Vec<&(Hash, BlockHeader)> = self.staker_active_blocks.iter().collect();
        blocks.sort_unstable_by_key(|v| (v.1.content.slot, v.0));
        for (hash, block) in &blocks {
            write!(
                f,
                "    block: hash:{} header: {}",
                hash,
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
                hash,
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

#[derive(Clone, Deserialize)]
pub struct NetworkInfo {
    our_ip: Option<IpAddr>,
    peers: HashMap<IpAddr, PeerInfo>,
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
        for peer in self.peers.values() {
            write!(f, "    {}", WrappedPeerInfo::from(peer))?;
        }
        Ok(())
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
