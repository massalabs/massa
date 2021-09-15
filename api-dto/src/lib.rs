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
    time_start: UTime,
    time_end: UTime,
    final_block_count: u64,
    stale_block_count: u64,
    final_operation_count: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PoolStats {
    operation_count: u64,
    endorsement_count: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NetworkStats {
    in_connection_count: u64,
    out_connection_count: u64,
    known_peer_count: u64,
    banned_peer_count: u64,
    active_node_count: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NodeStatus {
    node_id: NodeId,
    node_ip: Option<IpAddr>,
    version: Version,
    genesis_timestamp: UTime,
    t0: UTime,
    delta_f0: UTime,
    roll_price: Amount,
    thread_count: Amount,
    current_time: UTime,
    connected_nodes: HashMap<NodeId, IpAddr>,
    last_slot: Option<Slot>,
    next_slot: Slot,
    time_stats: TimeStats,
    pool_stats: PoolStats,
    network_stats: NetworkStats,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct OperationInfo {
    id: OperationId,
    in_pool: bool,
    in_blocks: Vec<BlockId>,
    is_final: bool,
    operation: Operation,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BalanceInfo {
    final_balance: Amount,
    candidate_balance: Amount,
    locked_balance: Amount,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RollsInfo {
    active_rolls: u64,
    final_rolls: u64,
    candidate_rolls: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AddressInfo {
    address: Address,
    thread: u8,
    balance: BalanceInfo,
    rolls: RollsInfo,
    block_draws: HashSet<Slot>,
    endorsement_draws: HashMap<Slot, u64>, // u64 is the index
    blocks_created: BlockHashSet,
    involved_in_endorsements: EndorsementHashSet,
    involved_in_operations: OperationHashSet,
    is_staking: bool,
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
    id: BlockId,
    is_final: bool,
    is_stale: bool,
    is_in_blockclique: bool,
    block: Block,
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
