// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::address::AddressCycleProductionStats;
use crate::ledger_models::LedgerData;
use crate::node::NodeId;
use crate::prehash::Map;
use crate::prehash::Set;
use crate::stats::{ConsensusStats, NetworkStats, PoolStats};
use crate::SignedEndorsement;
use crate::SignedOperation;
use crate::{
    Address, Amount, Block, BlockId, CompactConfig, EndorsementId, OperationId, Slot, Version,
};
use massa_hash::Hash;
use massa_time::MassaTime;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

/// node status
#[derive(Debug, Deserialize, Serialize)]
pub struct NodeStatus {
    /// our node id
    pub node_id: NodeId,
    /// optional node ip
    pub node_ip: Option<IpAddr>,
    /// node version
    pub version: Version,
    /// now
    pub current_time: MassaTime,
    /// current cycle
    pub current_cycle: u64,
    /// connected nodes (node id, ip address)
    pub connected_nodes: HashMap<NodeId, IpAddr>,
    /// latest slot, none if now is before genesis timestamp
    pub last_slot: Option<Slot>,
    /// next slot
    pub next_slot: Slot,
    /// consensus stats
    pub consensus_stats: ConsensusStats,
    /// pool stats
    pub pool_stats: PoolStats,
    /// network stats
    pub network_stats: NetworkStats,
    /// compact configuration
    pub config: CompactConfig,
}

impl std::fmt::Display for NodeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Node's ID: {}", self.node_id)?;
        if self.node_ip.is_some() {
            writeln!(f, "Node's IP: {}", self.node_ip.unwrap())?;
        } else {
            writeln!(f, "No routable IP set")?;
        }
        writeln!(f)?;

        writeln!(f, "Version: {}", self.version)?;
        writeln!(f, "Config:\n{}", self.config)?;
        writeln!(f)?;

        writeln!(f, "Current time: {}", self.current_time.to_utc_string())?;
        writeln!(f, "Current cycle: {}", self.current_cycle)?;
        if self.last_slot.is_some() {
            writeln!(f, "Last slot: {}", self.last_slot.unwrap())?;
        }
        writeln!(f, "Next slot: {}", self.next_slot)?;
        writeln!(f)?;

        writeln!(f, "{}", self.consensus_stats)?;

        writeln!(f, "{}", self.pool_stats)?;

        writeln!(f, "{}", self.network_stats)?;

        writeln!(f, "Connected nodes:")?;
        for (node_id, ip_addr) in &self.connected_nodes {
            writeln!(f, "\t[\"{}:31245\", \"{}\"],", ip_addr, node_id)?;
        }
        Ok(())
    }
}

/// Operation and contextual info about it
#[derive(Debug, Deserialize, Serialize)]
pub struct OperationInfo {
    /// id
    pub id: OperationId,
    /// true if operation is still in pool
    pub in_pool: bool,
    /// the operation appears in `in_blocks`
    /// if it appears in multiple blocks, these blocks are in different cliques
    pub in_blocks: Vec<BlockId>,
    /// true if the operation is final (for example in a final block)
    pub is_final: bool,
    /// the operation itself
    pub operation: SignedOperation,
}

impl OperationInfo {
    /// extend an operation info with another one
    /// There is not check to see if the id and operation are indeed the same
    pub fn extend(&mut self, other: &OperationInfo) {
        self.in_pool = self.in_pool || other.in_pool;
        self.in_blocks.extend(other.in_blocks.iter());
        self.is_final = self.is_final || other.is_final;
    }
}

impl std::fmt::Display for OperationInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Operation's ID: {}{}{}",
            self.id,
            display_if_true(self.in_pool, "in pool"),
            display_if_true(self.is_final, "final")
        )?;
        writeln!(f, "Block's ID")?;
        for block_id in &self.in_blocks {
            writeln!(f, "\t- {}", block_id)?;
        }
        writeln!(f, "{}", self.operation)?;
        Ok(())
    }
}

/// Current Parallel balance ledger info
#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
pub struct LedgerInfo {
    /// final data
    pub final_ledger_info: LedgerData,
    /// latest data
    pub candidate_ledger_info: LedgerData,
    /// locked balance, for example balance due to a roll sell
    pub locked_balance: Amount,
}

impl std::fmt::Display for LedgerInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "\tFinal balance: {}", self.final_ledger_info.balance)?;
        writeln!(
            f,
            "\tCandidate balance: {}",
            self.candidate_ledger_info.balance
        )?;
        writeln!(f, "\tLocked balance: {}", self.locked_balance)?;
        Ok(())
    }
}

/// Roll counts
#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
pub struct RollsInfo {
    /// count taken into account for the current cycle
    pub active_rolls: u64,
    /// at final blocks
    pub final_rolls: u64,
    /// at latest blocks
    pub candidate_rolls: u64,
}

impl std::fmt::Display for RollsInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "\tActive rolls: {}", self.active_rolls)?;
        writeln!(f, "\tFinal rolls: {}", self.final_rolls)?;
        writeln!(f, "\tCandidate rolls: {}", self.candidate_rolls)?;
        Ok(())
    }
}

/// Sequential balance state (really same as `SCELedgerEntry`)
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct SCELedgerInfo {
    /// sequential coins
    pub balance: Amount,
    /// stored bytes
    pub module: Vec<u8>,
    /// datastore
    pub datastore: Map<Hash, Vec<u8>>,
}

impl std::fmt::Display for SCELedgerInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "\tBalance: {}", self.balance)?;
        // I choose not to display neither the module nor the datastore because bytes
        Ok(())
    }
}

/// All you ever dream to know about an address
#[derive(Debug, Deserialize, Serialize)]
pub struct AddressInfo {
    /// the address
    pub address: Address,
    /// the thread it is in
    pub thread: u8,
    /// parallel balance info
    pub ledger_info: LedgerInfo,
    /// final sequential balance
    pub final_sce_ledger_info: SCELedgerInfo,
    /// latest sequential balance
    pub candidate_sce_ledger_info: SCELedgerInfo,
    /// rolls
    pub rolls: RollsInfo,
    /// next slots this address will be selected to create a block
    pub block_draws: HashSet<Slot>,
    /// next slots this address will be selected to create a endorsement
    pub endorsement_draws: HashSet<IndexedSlot>,
    /// created blocks ids
    pub blocks_created: Set<BlockId>,
    /// endorsements in which this address is involved (endorser, block creator)
    pub involved_in_endorsements: Set<EndorsementId>,
    /// operation in which this address is involved (sender or receiver)
    pub involved_in_operations: Set<OperationId>,
    /// stats about block production
    pub production_stats: Vec<AddressCycleProductionStats>,
}

impl std::fmt::Display for AddressInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Address: {}", self.address)?;
        writeln!(f, "Thread: {}", self.thread)?;
        writeln!(f, "Sequential balance:\n{}", self.ledger_info)?;
        writeln!(f, "Final Parallel balance:\n{}", self.final_sce_ledger_info)?;
        writeln!(
            f,
            "Candidate Parallel balance:\n{}",
            self.candidate_sce_ledger_info
        )?;
        writeln!(f, "Rolls:\n{}", self.rolls)?;
        writeln!(
            f,
            "Block draws: {}",
            self.block_draws
                .iter()
                .fold("\n".to_string(), |acc, s| format!("{}    {}", acc, s))
        )?;
        writeln!(
            f,
            "Endorsement draws: {}",
            self.endorsement_draws
                .iter()
                .fold("\n".to_string(), |acc, s| format!("{}    {}", acc, s))
        )?;
        writeln!(
            f,
            "Blocks created: {}",
            self.blocks_created
                .iter()
                .fold("\n".to_string(), |acc, s| format!("{}    {}", acc, s))
        )?;
        writeln!(
            f,
            "Involved in endorsements: {}",
            self.involved_in_endorsements
                .iter()
                .fold("\n".to_string(), |acc, s| format!("{}    {}", acc, s))
        )?;
        writeln!(
            f,
            "Involved in operations: {}",
            self.involved_in_operations
                .iter()
                .fold("\n".to_string(), |acc, s| format!("{}    {}", acc, s))
        )?;
        writeln!(f, "Production stats:")?;
        let mut sorted_production_stats = self.production_stats.clone();
        sorted_production_stats.sort_unstable_by_key(|v| v.cycle);
        for cycle_stat in sorted_production_stats.into_iter() {
            writeln!(
                f,
                "\t produced {} and failed {} at cycle {} {}",
                cycle_stat.ok_count,
                cycle_stat.nok_count,
                cycle_stat.cycle,
                if cycle_stat.is_final {
                    "(final)"
                } else {
                    "(non-final)"
                }
            )?;
        }

        Ok(())
    }
}

impl AddressInfo {
    /// Only essential info about an address
    pub fn compact(&self) -> CompactAddressInfo {
        CompactAddressInfo {
            address: self.address,
            thread: self.thread,
            balance: self.ledger_info,
            rolls: self.rolls,
            final_sce_balance: self.final_sce_ledger_info.clone(),
            candidate_sce_balance: self.candidate_sce_ledger_info.clone(),
        }
    }
}

/// When an address is drawn to create an endorsement it is selected for a specific index
#[derive(Debug, Deserialize, Serialize, Hash, PartialEq, Eq)]
pub struct IndexedSlot {
    /// slot
    pub slot: Slot,
    /// endorsement index in the slot
    pub index: usize,
}

impl std::fmt::Display for IndexedSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Slot: {}, Index: {}", self.slot, self.index)
    }
}

/// Less information about an address
#[derive(Debug, Serialize)]
pub struct CompactAddressInfo {
    /// the address
    pub address: Address,
    /// the thread it is
    pub thread: u8,
    /// parallel balance
    pub balance: LedgerInfo,
    /// rolls
    pub rolls: RollsInfo,
    /// final sequential balance
    pub final_sce_balance: SCELedgerInfo,
    /// latest sequential balance
    pub candidate_sce_balance: SCELedgerInfo,
}

impl std::fmt::Display for CompactAddressInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Address: {}", self.address)?;
        writeln!(f, "Thread: {}", self.thread)?;
        writeln!(f, "Final Sequential balance:\n{}", self.final_sce_balance)?;
        writeln!(
            f,
            "Candidate Sequential balance:\n{}",
            self.candidate_sce_balance
        )?;
        writeln!(f, "Parallel balance:\n{}", self.balance)?;
        writeln!(f, "Rolls:\n{}", self.rolls)?;
        Ok(())
    }
}

/// All you wanna know about an endorsement
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct EndorsementInfo {
    /// the id
    pub id: EndorsementId,
    /// true is the endorsement is still in pool
    pub in_pool: bool,
    /// endorsements included in these blocks
    pub in_blocks: Vec<BlockId>,
    /// true included in a final block
    pub is_final: bool,
    /// The full endorsement
    pub endorsement: SignedEndorsement,
}

impl std::fmt::Display for EndorsementInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Endorsement id: {}", self.id)?;
        display_if_true(self.is_final, "final");
        display_if_true(self.in_pool, "in pool");
        writeln!(
            f,
            "In blocks: {}",
            self.in_blocks
                .iter()
                .fold("\n".to_string(), |acc, s| format!("{}    {}", acc, s))
        )?;
        writeln!(f, "Endorsement: {}", self.endorsement)?;
        Ok(())
    }
}

/// refactor to delete
#[derive(Debug, Deserialize, Serialize)]
pub struct BlockInfo {
    /// block id
    pub id: BlockId,
    /// optional block info content
    pub content: Option<BlockInfoContent>,
}

/// Block content
#[derive(Debug, Deserialize, Serialize)]
pub struct BlockInfoContent {
    /// true if final
    pub is_final: bool,
    /// true if incompatible with a final block
    pub is_stale: bool,
    /// true if in the greatest clique
    pub is_in_blockclique: bool,
    /// block
    pub block: Block,
}

impl std::fmt::Display for BlockInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(content) = &self.content {
            writeln!(
                f,
                "Block's ID: {}{}{}{}",
                self.id,
                display_if_true(content.is_final, "final"),
                display_if_true(content.is_stale, "stale"),
                display_if_true(content.is_in_blockclique, "in blockclique"),
            )?;
            writeln!(f, "Block: {}", content.block)?;
        } else {
            writeln!(f, "Block {} not found", self.id)?;
        }
        Ok(())
    }
}

/// A block resume (without the block itself)
#[derive(Debug, Deserialize, Serialize)]
pub struct BlockSummary {
    /// id
    pub id: BlockId,
    /// true if in a final block
    pub is_final: bool,
    /// true if incompatible with a final block
    pub is_stale: bool,
    /// true if in the greatest block clique
    pub is_in_blockclique: bool,
    /// the slot the block is in
    pub slot: Slot,
    /// the block creator
    pub creator: Address,
    /// the block parents
    pub parents: Vec<BlockId>,
}

impl std::fmt::Display for BlockSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Block's ID: {}{}{}{}",
            self.id,
            display_if_true(self.is_final, "final"),
            display_if_true(self.is_stale, "stale"),
            display_if_true(self.is_in_blockclique, "in blockclique"),
        )?;
        writeln!(f, "Slot: {}", self.slot)?;
        writeln!(f, "Creator: {}", self.creator)?;
        writeln!(f, "Parents' IDs:")?;
        for parent in &self.parents {
            writeln!(f, "\t- {}", parent)?;
        }
        Ok(())
    }
}

/// Dumb utils function to display nicely boolean value
fn display_if_true(value: bool, text: &str) -> String {
    if value {
        format!("[{}]", text)
    } else {
        String::from("")
    }
}

/// Just a wrapper with a optional beginning and end
#[derive(Debug, Deserialize, Clone, Copy, Serialize)]
pub struct TimeInterval {
    /// optional start slot
    pub start: Option<MassaTime>,
    /// optional end slot
    pub end: Option<MassaTime>,
}

/// filter used when retrieving SC output events
#[derive(Default, Debug, Deserialize, Clone, Serialize)]
pub struct EventFilter {
    /// optional start slot
    pub start: Option<Slot>,
    /// optional end slot
    pub end: Option<Slot>,
    /// optional emitter address
    pub emitter_address: Option<Address>,
    /// optional caller address
    pub original_caller_address: Option<Address>,
    /// optional operation id
    pub original_operation_id: Option<OperationId>,
}

/// read only bytecode execution request
#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct ReadOnlyBytecodeExecution {
    /// max available gas
    pub max_gas: u64,
    /// gas price
    pub simulated_gas_price: Amount,
    /// byte code
    pub bytecode: Vec<u8>,
    /// caller's address, optional
    pub address: Option<Address>,
}

/// read SC call request
#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct ReadOnlyCall {
    /// max available gas
    pub max_gas: u64,
    /// gas price
    pub simulated_gas_price: Amount,
    /// target address
    pub target_address: Address,
    /// target function
    pub target_function: String,
    /// function parameter
    pub parameter: String,
    /// caller's address, optional
    pub caller_address: Option<Address>,
}
