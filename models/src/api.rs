// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::address::AddressCycleProductionStats;
use crate::ledger::LedgerData;
use crate::node::NodeId;
use crate::stats::{ConsensusStats, NetworkStats, PoolStats};
use crate::{
    Address, AlgoConfig, Amount, Block, BlockHashSet, BlockId, Endorsement, EndorsementHashSet,
    EndorsementId, Operation, OperationHashSet, OperationId, Slot, Version,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use time::UTime;

#[derive(Debug, Deserialize, Serialize)]
pub struct NodeStatus {
    pub node_id: NodeId,
    pub node_ip: Option<IpAddr>,
    pub version: Version,
    pub genesis_timestamp: UTime,
    pub t0: UTime,
    pub delta_f0: u64,
    pub roll_price: Amount,
    pub thread_count: u8,
    pub current_time: UTime,
    pub current_cycle: u64,
    pub connected_nodes: HashMap<NodeId, IpAddr>,
    pub last_slot: Option<Slot>,
    pub next_slot: Slot,
    pub consensus_stats: ConsensusStats,
    pub pool_stats: PoolStats,
    pub network_stats: NetworkStats,
    pub algo_config: AlgoConfig,
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
        writeln!(
            f,
            "Genesis timestamp: {}",
            self.genesis_timestamp.to_utc_string()
        )?;
        writeln!(f, "t0: {}ms", self.t0.to_duration().as_millis())?;
        writeln!(f, "delta f0: {}", self.delta_f0)?;
        writeln!(f, "Roll price: {}", self.roll_price)?;
        writeln!(f, "Thread count: {}", self.thread_count)?;
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
            writeln!(f, "\tNode's ID: {} / IP address: {}", node_id, ip_addr)?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct OperationInfo {
    pub id: OperationId,
    pub in_pool: bool,
    pub in_blocks: Vec<BlockId>,
    pub is_final: bool,
    pub operation: Operation,
}

impl OperationInfo {
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

#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
pub struct LedgerInfo {
    pub final_ledger_info: LedgerData,
    pub candidate_ledger_info: LedgerData,
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

#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
pub struct RollsInfo {
    pub active_rolls: u64,
    pub final_rolls: u64,
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

#[derive(Debug, Deserialize, Serialize)]
pub struct AddressInfo {
    pub address: Address,
    pub thread: u8,
    pub ledger_info: LedgerInfo,
    pub rolls: RollsInfo,
    pub block_draws: HashSet<Slot>,
    pub endorsement_draws: HashSet<IndexedSlot>,
    pub blocks_created: BlockHashSet,
    pub involved_in_endorsements: EndorsementHashSet,
    pub involved_in_operations: OperationHashSet,
    pub production_stats: Vec<AddressCycleProductionStats>,
}

impl std::fmt::Display for AddressInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Address: {}", self.address)?;
        writeln!(f, "Thread: {}", self.thread)?;
        writeln!(f, "Balance:\n{}", self.ledger_info)?;
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
    pub fn compact(&self) -> CompactAddressInfo {
        CompactAddressInfo {
            address: self.address,
            thread: self.thread,
            balance: self.ledger_info,
            rolls: self.rolls,
        }
    }
}

/// When an address is drawn to create an endorsement it is selected for a specific index
#[derive(Debug, Deserialize, Serialize, Hash, PartialEq, Eq)]
pub struct IndexedSlot {
    pub slot: Slot,
    pub index: usize,
}

impl std::fmt::Display for IndexedSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Slot: {}, Index: {}", self.slot, self.index)
    }
}

#[derive(Serialize)]
pub struct CompactAddressInfo {
    pub address: Address,
    pub thread: u8,
    pub balance: LedgerInfo,
    pub rolls: RollsInfo,
}

impl std::fmt::Display for CompactAddressInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Address: {}", self.address)?;
        writeln!(f, "Thread: {}", self.thread)?;
        writeln!(f, "Balance:\n{}", self.balance)?;
        writeln!(f, "Rolls:\n{}", self.rolls)?;
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct EndorsementInfo {
    pub id: EndorsementId,
    pub in_pool: bool,
    pub in_blocks: Vec<BlockId>,
    pub is_final: bool,
    pub endorsement: Endorsement,
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

#[derive(Debug, Deserialize, Serialize)]
pub struct BlockInfo {
    pub id: BlockId,
    pub content: Option<BlockInfoContent>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BlockInfoContent {
    pub is_final: bool,
    pub is_stale: bool,
    pub is_in_blockclique: bool,
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

#[derive(Debug, Deserialize, Serialize)]
pub struct BlockSummary {
    pub id: BlockId,
    pub is_final: bool,
    pub is_stale: bool,
    pub is_in_blockclique: bool,
    pub slot: Slot,
    pub creator: Address,
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

#[derive(Debug, Deserialize, Clone, Copy, Serialize)]
pub struct TimeInterval {
    pub start: Option<UTime>,
    pub end: Option<UTime>,
}

#[derive(Debug, Deserialize, Clone, Copy)]
pub struct APISettings {
    pub draw_lookahead_period_count: u64,
    pub bind_private: SocketAddr,
    pub bind_public: SocketAddr,
    pub max_arguments: u64,
}
