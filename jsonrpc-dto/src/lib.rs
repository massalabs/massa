// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crypto::signature::{PrivateKey, PublicKey, Signature};
use jsonrpc_core::Result;
use jsonrpc_derive::rpc;
use models::address::{AddressHashMap, AddressHashSet};
use models::clique::Clique;
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

/// Public Ethereum JSON-RPC endpoints (in intend to be compatible with EthRpc.io)
#[rpc(server)]
pub trait EthRpc {
    /// Will be implemented later, when smart contracts are added.
    #[rpc(name = "Call")]
    fn call(&self) -> Result<()>;

    #[rpc(name = "getBalance")]
    fn get_balance(&self) -> Result<Amount>;

    #[rpc(name = "sendTransaction")]
    fn send_transaction(&self) -> Result<()>;

    #[rpc(name = "HelloWorld")]
    fn hello_world(&self) -> Result<String>;
}

/// Public Massa JSON-RPC endpoints
#[rpc(server)]
pub trait MassaPublic {
    /////////////////////////////////
    // Explorer (aggregated stats) //
    /////////////////////////////////

    /// summary of the current state: time, last final blocks (hash, thread, slot, timestamp), clique count, connected nodes count
    #[rpc(name = "get_status")]
    fn get_status(&self) -> Result<NodeStatus>;

    #[rpc(name = "get_cliques")]
    fn get_cliques(&self) -> Result<Vec<Clique>>;

    //////////////////////////////////
    // Debug (specific information) //
    //////////////////////////////////

    /// Returns the active stakers and their roll counts for the current cycle.
    #[rpc(name = "get_stakers")]
    fn get_stakers(&self) -> Result<AddressHashMap<u64>>;

    /// Returns operations information associated to a given list of operations' IDs.
    #[rpc(name = "get_operations")]
    fn get_operations(&self, _: Vec<OperationId>) -> Result<Vec<OperationInfo>>;

    #[rpc(name = "get_endorsements")]
    fn get_endorsements(&self, _: Vec<EndorsementId>) -> Result<Vec<EndorsementInfo>>;

    /// Get information on a block given its hash
    #[rpc(name = "get_block")]
    fn get_block(&self, _: BlockId) -> Result<BlockInfo>;

    /// Get the block graph within the specified time interval.
    /// Optional parameters: from <time_start> (included) and to <time_end> (excluded) millisecond timestamp
    #[rpc(name = "get_graph_interval")]
    fn get_graph_interval(
        &self,
        time_start: Option<UTime>,
        time_end: Option<UTime>,
    ) -> Result<Vec<BlockSummary>>;

    //////////////////////////////////////
    // User (interaction with the node) //
    //////////////////////////////////////

    /// Return list of all those that were sent
    #[rpc(name = "send_operations")]
    fn send_operations(&self, _: Vec<Operation>) -> Result<Vec<OperationId>>;
}

/// Private Massa-RPC "manager mode" endpoints
#[rpc(server)]
pub trait MassaPrivate {
    /// Starts the node and waits for node to start.
    /// Signals if the node is already running.
    #[rpc(name = "start_node")]
    fn start_node(&self) -> Result<()>;

    /// Gracefully stop the node.
    #[rpc(name = "stop_node")]
    fn stop_node(&self) -> Result<()>;

    #[rpc(name = "sign_message")]
    fn sign_message(&self, _: Vec<u8>) -> Result<(Signature, PublicKey)>;

    /// Add a new private key for the node to use to stake.
    #[rpc(name = "add_staking_keys")]
    fn add_staking_keys(&self, _: Vec<PrivateKey>) -> Result<()>;

    /// Remove an address used to stake.
    #[rpc(name = "remove_staking_keys")]
    fn remove_staking_keys(&self, _: Vec<Address>) -> Result<()>;

    /// Return hashset of staking addresses.
    #[rpc(name = "list_staking_keys")]
    fn list_staking_keys(&self) -> Result<AddressHashSet>;

    #[rpc(name = "ban")]
    fn ban(&self, _: NodeId) -> Result<()>;

    #[rpc(name = "unban")]
    fn unban(&self, _: IpAddr) -> Result<()>;

    #[rpc(name = "get_addresses")]
    fn get_addresses(&self, _: Vec<Address>) -> Result<Vec<AddressInfo>>;
}

pub struct API;

impl EthRpc for API {
    fn call(&self) -> Result<()> {
        todo!()
    }

    fn get_balance(&self) -> Result<Amount> {
        todo!()
    }

    fn send_transaction(&self) -> Result<()> {
        todo!()
    }

    fn hello_world(&self) -> Result<String> {
        Ok(String::from("Hello, World!"))
    }
}

impl MassaPublic for API {
    fn get_status(&self) -> Result<NodeStatus> {
        todo!()
    }

    fn get_cliques(&self) -> Result<Vec<Clique>> {
        todo!()
    }

    fn get_stakers(&self) -> Result<AddressHashMap<u64>> {
        todo!()
    }

    fn get_operations(&self, _: Vec<OperationId>) -> Result<Vec<OperationInfo>> {
        todo!()
    }

    fn get_endorsements(&self, _: Vec<EndorsementId>) -> Result<Vec<EndorsementInfo>> {
        todo!()
    }

    fn get_block(&self, _: BlockId) -> Result<BlockInfo> {
        todo!()
    }

    fn get_graph_interval(
        &self,
        _time_start: Option<UTime>,
        _time_end: Option<UTime>,
    ) -> Result<Vec<BlockSummary>> {
        todo!()
    }

    fn send_operations(&self, _: Vec<Operation>) -> Result<Vec<OperationId>> {
        todo!()
    }
}

impl MassaPrivate for API {
    fn start_node(&self) -> Result<()> {
        todo!()
    }

    fn stop_node(&self) -> Result<()> {
        todo!()
    }

    fn sign_message(&self, _: Vec<u8>) -> Result<(Signature, PublicKey)> {
        todo!()
    }

    fn add_staking_keys(&self, _: Vec<PrivateKey>) -> Result<()> {
        todo!()
    }

    fn remove_staking_keys(&self, _: Vec<Address>) -> Result<()> {
        todo!()
    }

    fn list_staking_keys(&self) -> Result<AddressHashSet> {
        todo!()
    }

    fn ban(&self, _: NodeId) -> Result<()> {
        todo!()
    }

    fn unban(&self, _: IpAddr) -> Result<()> {
        todo!()
    }

    fn get_addresses(&self, _: Vec<Address>) -> Result<Vec<AddressInfo>> {
        todo!()
    }
}
