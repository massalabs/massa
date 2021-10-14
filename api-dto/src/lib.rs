// Copyright (c) 2021 MASSA LABS <info@massa.net>

pub mod config;

use communication::network::NetworkStats;
use models::node::NodeId;
use models::{
    Address, Amount, Block, BlockHashSet, BlockId, Endorsement, EndorsementHashSet, EndorsementId,
    Operation, OperationHashSet, OperationId, Slot, Version,
};
use pool::PoolStats;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use time::UTime;

#[derive(Debug, Deserialize, Serialize)]
pub struct TimeStats {
    pub time_start: UTime,
    pub time_end: Option<UTime>,
    pub final_block_count: u64,
    pub stale_block_count: u64,
    pub final_operation_count: u64,
}

impl std::fmt::Display for TimeStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Start time: {}", self.time_start)?;
        if self.time_end.is_some() {
            writeln!(f, "End time: {}", self.time_end.unwrap())?;
        }
        writeln!(f, "Final block count: {}", self.final_block_count)?;
        writeln!(f, "Stale block count: {}", self.stale_block_count)?;
        writeln!(f, "Final operation count: {}", self.final_operation_count)?;
        Ok(())
    }
}

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
    pub connected_nodes: HashMap<NodeId, IpAddr>,
    pub last_slot: Option<Slot>,
    pub next_slot: Slot,
    pub time_stats: TimeStats,
    pub pool_stats: PoolStats,
    pub network_stats: NetworkStats,
}

impl std::fmt::Display for NodeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Node's ID: {}", self.node_id)?;
        if self.node_ip.is_some() {
            writeln!(f, "Node's IP: {}", self.node_ip.unwrap())?;
        }
        writeln!(f, "Version: {}", self.version)?;
        writeln!(f, "Genesis timestamp: {}", self.genesis_timestamp)?;
        writeln!(f, "t0={} / delta f0={}", self.t0, self.delta_f0)?;
        writeln!(f, "Roll price: {}", self.roll_price)?;
        writeln!(f, "Thread count: {}", self.thread_count)?;
        writeln!(f, "Current time: {}", self.current_time)?;
        writeln!(f, "Connected nodes: {:?}", self.connected_nodes)?; // TODO
        if self.last_slot.is_some() {
            writeln!(f, "Last slot: {}", self.last_slot.unwrap())?;
        }
        writeln!(f, "Next slot: {}", self.next_slot)?;
        writeln!(f, "Time stats:\n{}", self.time_stats)?;
        writeln!(f, "Pool stats:\n{}", self.pool_stats)?;
        writeln!(f, "Network stats:\n{}", self.network_stats)?;
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

#[derive(Debug, Deserialize, Serialize)]
pub struct BalanceInfo {
    pub final_balance: Amount,
    pub candidate_balance: Amount,
    pub locked_balance: Amount,
}

impl std::fmt::Display for BalanceInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Final balance: {}", self.final_balance)?;
        writeln!(f, "Candidate balance: {}", self.candidate_balance)?;
        writeln!(f, "Locked balance: {}", self.locked_balance)?;
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RollsInfo {
    pub active_rolls: u64,
    pub final_rolls: u64,
    pub candidate_rolls: u64,
}

impl std::fmt::Display for RollsInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Active rolls: {}", self.active_rolls)?;
        writeln!(f, "Final rolls: {}", self.final_rolls)?;
        writeln!(f, "Candidate rolls: {}", self.candidate_rolls)?;
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AddressInfo {
    pub address: Address,
    pub thread: u8,
    pub balance: BalanceInfo,
    pub rolls: RollsInfo,
    pub block_draws: HashSet<Slot>,
    pub endorsement_draws: HashMap<Slot, u64>, // u64 is the index
    pub blocks_created: BlockHashSet,
    pub involved_in_endorsements: EndorsementHashSet,
    pub involved_in_operations: OperationHashSet,
    pub is_staking: bool,
}

impl std::fmt::Display for AddressInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Address: {}{}",
            self.address,
            display_if_true(self.is_staking, "staking"),
        )?;
        writeln!(f, "Thread: {}", self.thread)?;
        writeln!(f, "Balance:\n{}", self.balance)?;
        writeln!(f, "Rolls:\n{}", self.rolls)?;
        writeln!(f, "Block draws: {:?}", self.block_draws)?; // TODO
        writeln!(f, "Endorsement draws: {:?}", self.endorsement_draws)?; // TODO
        writeln!(f, "Blocks created: {:?}", self.blocks_created)?; // TODO
        writeln!(
            f,
            "Involved in endorsements: {:?}",
            self.involved_in_endorsements
        )?; // TODO
        writeln!(
            f,
            "Involved in operations: {:?}",
            self.involved_in_operations
        )?; // TODO
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct EndorsementInfo {
    id: EndorsementId,
    in_pool: bool,
    in_blocks: Vec<BlockId>,
    is_final: bool,
    endorsement: Endorsement,
}

impl std::fmt::Display for EndorsementInfo {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!() // TODO: wait for !238
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BlockInfo {
    pub id: BlockId,
    pub is_final: bool,
    pub is_stale: bool,
    pub is_in_blockclique: bool,
    pub block: Block,
}

impl std::fmt::Display for BlockInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Block's ID: {}{}{}{}",
            self.id,
            display_if_true(self.is_final, "final"),
            display_if_true(self.is_stale, "stale"),
            display_if_true(self.is_in_blockclique, "in blockclique"),
        )?;
        writeln!(f, "Block: {}", self.block)?;
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
