//!Contains all the structure receive using the REST API.
//!
//!All struct implements display to be shown on the screen.
//!
//! Copy of all structure defined in the API side. They has been copied to avoid to force some behaviour on the massa node developements like display.
//! To detect desynchronisation between the 2 API, tests has been added to validated the deserialisation of the REST API call response.
//!
//! There're only deserialized when received from the REST call.

//massa type are wrapped to define a client specific display behaviour.
//The display method is only use to show the data REPL mode.

use chrono::Local;
use chrono::TimeZone;
use communication::network::PeerInfo;
use consensus::DiscardReason;
use consensus::{AddressState, ExportBlockStatus, LedgerData};
use crypto::hash::Hash;
use crypto::signature::Signature;
use models::{
    Address, Block, BlockHeader, BlockId, Operation, OperationSearchResultBlockStatus,
    OperationSearchResultStatus, OperationType, Slot,
};
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
            OperationType::RollBuy { roll_count } => {
                write!(f, "RollBuy: roll_count:{}", roll_count)
            }
            OperationType::RollSell { roll_count } => {
                write!(f, "RollSell: roll_count:{}", roll_count)
            }
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
            "sender:{} fee:{} expire_period:{} {}",
            addr, self.0.content.fee, self.0.content.expire_period, op_type
        )
    }
}

#[derive(Clone, Debug)]
pub struct OperationSearchResultStatusWrapper<'a>(&'a OperationSearchResultStatus);

impl<'a> std::fmt::Display for OperationSearchResultStatusWrapper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let status = match &self.0 {
            OperationSearchResultStatus::Pending => "Pending",
            OperationSearchResultStatus::InBlock(block_status) => match block_status {
                OperationSearchResultBlockStatus::Incoming => "InBlock(Incoming)",
                OperationSearchResultBlockStatus::WaitingForSlot => "InBlock(WaitingForSlot)",
                OperationSearchResultBlockStatus::WaitingForDependencies => {
                    "InBlock(WaitingForDependencies)"
                }
                OperationSearchResultBlockStatus::Active => "InBlock(Active)",
                OperationSearchResultBlockStatus::Discarded => "InBlock(Discarded)",
                OperationSearchResultBlockStatus::Stored => "InBlock(Stored)",
            },
            OperationSearchResultStatus::Discarded => "Discarded",
        };
        write!(f, "{}", status)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct GetOperationContent {
    pub op: WrapperOperation,
    pub in_pool: bool,
    pub in_blocks: HashMap<BlockId, (usize, bool)>,
    pub status: OperationSearchResultStatus,
}

impl std::fmt::Display for GetOperationContent {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(
            f,
            "{} status:{} in pool:{}",
            OperationSearchResultStatusWrapper(&self.status),
            self.op,
            self.in_pool
        )?;
        writeln!(
            f,
            "block list:{}",
            self.in_blocks
                .iter()
                .map(|(id, (_idx, f))| format!("({}, final:{})", id, f))
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

/// Wrapps a (hash, slot)
pub fn from_hash_slot((hash, slot): (Hash, Slot)) -> (WrappedHash, WrappedSlot) {
    (hash.into(), slot.into())
}

/// Wrapps a vec of (hash, slot)
pub fn from_vec_hash_slot(list: &[(Hash, Slot)]) -> Vec<(WrappedHash, WrappedSlot)> {
    list.into_iter().map(|v| from_hash_slot(*v)).collect()
}

/// Wrapps a ledger data export
pub fn extract_addresses_from_ledger<'a>(
    ledger: &'a HashMap<Address, AddressState>,
    ordered_addrs: Option<Vec<Address>>,
) -> Vec<WrapperAddressLedgerDataExport<'a>> {
    //extract address from final_data
    let mut base_ledger_map: HashMap<&Address, WrapperAddressLedger<'a>> = ledger
        .iter()
        .map(|(addr, state)| {
            (
                addr,
                WrapperAddressLedger {
                    final_balance: Some(&state.final_ledger_data),
                    candidate_balance: None,
                },
            )
        })
        .collect();
    //get balance at best parents.
    ledger.iter().for_each(|(addr, state)| {
        let mut data = base_ledger_map
            .entry(addr)
            .or_insert_with(|| WrapperAddressLedger {
                final_balance: None,
                candidate_balance: None,
            });
        data.candidate_balance = Some(&state.candidate_ledger_data);
    });

    if let Some(ord) = ordered_addrs.clone() {
        ord.into_iter()
            .map(|(address)| WrapperAddressLedgerDataExport {
                address: address,
                balances: base_ledger_map.get(&address).unwrap().clone(),
            })
            .collect()
    } else {
        base_ledger_map
            .into_iter()
            .map(|(address, balances)| WrapperAddressLedgerDataExport {
                address: *address,
                balances,
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct WrapperAddressLedger<'a> {
    pub final_balance: Option<&'a LedgerData>,
    pub candidate_balance: Option<&'a LedgerData>,
}

#[derive(Debug, Clone)]
pub struct WrapperAddressLedgerDataExport<'a> {
    pub address: Address,
    pub balances: WrapperAddressLedger<'a>,
}

impl<'a> std::fmt::Display for WrapperAddressLedgerDataExport<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "address: {}", self.address)?;
        if let Some(balance) = self.balances.final_balance {
            write!(f, ", final balance: {}", balance.balance)?;
        }
        if let Some(balance) = self.balances.candidate_balance {
            write!(f, ", candidate balance: {}", balance.balance)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub enum WrappedBlockStatus {
    Incoming,
    WaitingForSlot,
    WaitingForDependencies,
    Active(WrapperBlock),
    Discarded(DiscardReason),
    Stored(WrapperBlock),
}

impl From<ExportBlockStatus> for WrappedBlockStatus {
    fn from(block: ExportBlockStatus) -> Self {
        match block {
            ExportBlockStatus::Incoming => WrappedBlockStatus::Incoming,
            ExportBlockStatus::WaitingForSlot => WrappedBlockStatus::WaitingForSlot,
            ExportBlockStatus::WaitingForDependencies => WrappedBlockStatus::WaitingForDependencies,
            ExportBlockStatus::Active(block) => WrappedBlockStatus::Active(block.into()),
            ExportBlockStatus::Discarded(reason) => WrappedBlockStatus::Discarded(reason),
            ExportBlockStatus::Stored(block) => WrappedBlockStatus::Stored(block.into()),
        }
    }
}

impl std::fmt::Display for WrappedBlockStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match &self {
            WrappedBlockStatus::Incoming => write!(f, "status: Incoming"),
            WrappedBlockStatus::WaitingForSlot => write!(f, "status: WaitingForSlot"),
            WrappedBlockStatus::WaitingForDependencies => {
                write!(f, "status: WaitingForDependencies")
            }
            WrappedBlockStatus::Active(block) => write!(f, "status: Active, {}", block),
            WrappedBlockStatus::Discarded(reason) => write!(f, "status: Discarded({:?})", reason),
            WrappedBlockStatus::Stored(block) => write!(f, "status: Stored, {}", block),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct WrapperBlock {
    pub header: WrappedBlockHeader,
    pub operations: Vec<WrapperOperation>,
    pub signature: Signature,
}

impl From<Block> for WrapperBlock {
    fn from(block: Block) -> Self {
        WrapperBlock {
            operations: block.operations.into_iter().map(|op| op.into()).collect(),
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
        write!(
            f,
            "{} signature:{} operations:{}",
            self.header,
            signature,
            self.operations
                .iter()
                .map(|op| format!("({}", op))
                .collect::<Vec<String>>()
                .join(" ")
        )
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
            "creator: {} period: {} thread: {} merkle_root: {} parents: {:?}",
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
    current_cycle: u64,
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
            "  Time: {:?} Latest:{} Cycle:{}\n",
            date,
            self.latest_slot
                .map(|s| format!("Slot {:?}", s))
                .unwrap_or("None".to_string()),
            self.current_cycle
        )?;
        write!(
            f,
            " Nb peers: {}, our IP: {}\n",
            self.nb_peers,
            self.our_ip
                .map(|i| i.to_string())
                .unwrap_or("None".to_string())
        )?;
        let mut final_blocks: Vec<&(Hash, Slot, UTime)> = self.last_final.iter().collect();
        final_blocks.sort_unstable_by_key(|v| (v.1, v.0));

        write!(
            f,
            " Nb cliques: {}, last final blocks:{:#?}\n",
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
pub struct NextDraws(Vec<(Address, Slot)>);

impl std::fmt::Display for NextDraws {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "next draws :")?;
        for (addr, slot) in self.0.iter() {
            writeln!(f, "draw: address: {} slot: {}", addr, slot)?
        }
        Ok(())
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
            "  staker_next_draws: {:?}",
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
        writeln!(f, "Peer: Ip: {} bootstrap: {} banned: {} last_alive: {} last_failure: {} act_out_attempts: {} act_out: {} act_in: {} advertised:{}"
            ,self.0.ip
            , self.0.bootstrap
            , self.0.banned
            , self.0.last_alive.map(|t| format!("{:?}",Local.timestamp(Into::<Duration>::into(t).as_secs() as i64, 0))).unwrap_or("None".to_string())
            , self.0.last_failure.map(|t| format!("{:?}",Local.timestamp(Into::<Duration>::into(t).as_secs() as i64, 0))).unwrap_or("None".to_string())
            , self.0.active_out_connection_attempts
            , self.0.active_out_connections
            , self.0.active_in_connections
            , self.0.advertised)
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
