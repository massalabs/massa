// Copyright (c) 2021 MASSA LABS <info@massa.net>

use models::node::NodeId;
use models::{
    Address, Amount, Block, BlockHashSet, BlockId, Endorsement, EndorsementHashSet, EndorsementId,
    Operation, OperationHashSet, OperationId, Slot, Version,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use time::UTime;

#[derive(Serialize, Deserialize, Debug)]
pub struct TimeStats {
    pub time_start: UTime,
    pub time_end: Option<UTime>,
    pub final_block_count: u64,
    pub stale_block_count: u64,
    pub final_operation_count: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PoolStats {
    pub operation_count: u64,
    pub endorsement_count: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NetworkStats {
    pub in_connection_count: u64,
    pub out_connection_count: u64,
    pub known_peer_count: u64,
    pub banned_peer_count: u64,
    pub active_node_count: u64,
}

#[derive(Serialize, Deserialize, Debug)]
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

#[derive(Serialize, Deserialize, Debug)]
pub struct OperationInfo {
    pub id: OperationId,
    pub in_pool: bool,
    pub in_blocks: Vec<BlockId>,
    pub is_final: bool,
    pub operation: Operation,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BalanceInfo {
    pub final_balance: Amount,
    pub candidate_balance: Amount,
    pub locked_balance: Amount,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RollsInfo {
    pub active_rolls: u64,
    pub final_rolls: u64,
    pub candidate_rolls: u64,
}

#[derive(Serialize, Deserialize, Debug)]
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

#[derive(Serialize, Deserialize, Debug)]
pub struct EndorsementInfo {
    id: EndorsementId,
    in_pool: bool,
    in_blocks: Vec<BlockId>,
    is_final: bool,
    endorsement: Endorsement,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BlockInfo {
    pub id: BlockId,
    pub is_final: bool,
    pub is_stale: bool,
    pub is_in_blockclique: bool,
    pub block: Block,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BlockSummary {
    pub id: BlockId,
    pub is_final: bool,
    pub is_stale: bool,
    pub is_in_blockclique: bool,
    pub slot: Slot,
    pub creator: Address,
    pub parents: Vec<BlockId>,
}
