use communication::NodeId;
use consensus::Clique;
use crypto::signature::{PrivateKey, PublicKey, Signature};
use jsonrpc_core::Result;
use jsonrpc_derive::rpc;
use models::{
    Address, Amount, Block, BlockId, Endorsement, EndorsementId, Operation, OperationId, Slot,
    Version,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use time::UTime;

// TODO:
// * `get_node_config -> SerializationContext`
// * `get_pool_config -> PoolConfig`
// * `get_consensus_config -> ConsensusConfig`
#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    // FIXME: Duplicated?
    pub t0: UTime,
    pub delta_f0: u64,
    pub version: Version,
    pub genesis_timestamp: UTime,
    pub roll_price: Amount, // TODO: architecture params
}

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
    candidate: Amount,
    locked: Amount,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RollsInfo {
    active: u64,
    final_rolls: u64,
    candidate: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AddressInfo {
    address: Address,
    thread: u8,
    balance: BalanceInfo,
    rolls: RollsInfo,
    block_draws: Vec<Slot>,
    endorsement_draws: HashMap<Slot, u64>, // u64 is the index
    blocks_created: Vec<BlockId>,
    involved_in_endorsements: Vec<EndorsementId>,
    involved_in_operations: Vec<OperationId>,
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

#[derive(Serialize, Deserialize, Debug)]
pub struct StatusCode(u8); // TODO

/// Public Ethereum JSON-RPC endpoints (in intend to be compatible with MetaMask.io)
#[rpc(server)]
pub trait MetaMask {
    /// Will be implemented later, when smart contracts are added.
    #[rpc(name = "Call")]
    fn call(&self) -> Result<StatusCode>;

    #[rpc(name = "getBalance")]
    fn get_balance(&self) -> Result<Amount>;

    #[rpc(name = "sendTransaction")]
    fn send_transaction(&self) -> Result<StatusCode>;
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
    fn get_stakers(&self) -> Result<HashMap<Address, u64>>;

    /// Returns operations information associated to a given list of operations' IDs.
    #[rpc(name = "get_operations")]
    fn get_operations(&self, _: Vec<OperationId>) -> Result<Vec<OperationInfo>>;

    #[rpc(name = "get_addresses")]
    fn get_addresses(&self, _: Vec<Address>) -> Result<Vec<AddressInfo>>;

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
    /// TODO: did this create an operation?
    #[rpc(name = "send_operations")]
    fn send_operations(&self, _: Vec<Operation>) -> Result<Vec<OperationId>>;

    // TODO:
    //     - `get_node_info`: We should fuse `our_ip`, `peers`, `get_network_info`, `get_config` into a single node_info command:
    //         - inputs (None)
    //         - output NodeInfo with fields:
    //             - node_id: NodeId
    //             - node_ip: Option<IpAddress>
    //             - version: Version
    //             - genesis_timestamp: UTime
    //             - t0
    //             - delta_f0
    //             - roll_price
    //             - thread_count
    //             - connected_nodes: HashMap<NodeId, IpAddress>
    //     - `version`: current node version
    //     - `operations_involving_address`: list operations involving the provided address. Note that old operations are forgotten. `operations_involving_address -> HashMap<OperationId, OperationSearchResult>`
    //     - `block_ids_by_creator`: list blocks created by the provided address. Note that old blocks are forgotten.
    //     - `staker_stats`: production stats from staker address. Parameters: list of addresses.
    //     - `get_endorsement_by_id` -> endorsement state { id: EndorsementId, in_pool: bool, in_blocks: [BlockId] list, is_final: bool, endorsement: full Endorsement object }`
    //     - `addresses_info`: returns the final and candidate balances for a list of addresses. Parameters: list of addresses separated by, (no space). `addresses_info(Vec<Address>) -> HashMap<Address, AddressState>`
    //     - `staker_info`: staker info from staker address -> (blocks created, next slots in which the address will be selected) `staker_info -> StakerInfo { staker_active_blocks: Vec<(BlockId, BlockHeader)>, staker_discarded_blocks: Vec<(BlockId, DiscardReason, BlockHeader)>, staker_next_draws: Vec }`
    //     - `get_next_draw` (block and endorsement creation) next draws for given addresses (list of addresses separated by, (no space)) -> vec (address, slot for which address is selected) `next_draws(Vec<Address>) -> Vec<(Address, Slot)>`
    //         - input [Address] list
    //         - output : [slot] list
    //     - `get_balances_by_address`:
    //         - input [Address] list
    //         - output : for each address
    //             - candidate balance : 64
    //             - final balance : u64
    //             - locked balance : u64
    //             - candidate roll count : u64
    //             - final roll count : u64
    //     - `get_genesis`: `time_since_to_genesis() -> i64`
    //         - input none
    //         - relative time since/to genesis timestamp (in slots ?)
}

/// Private Massa-RPC "manager mode" endpoints
#[rpc(server)]
pub trait MassaPrivate {
    /// Starts the node and waits for node to start.
    /// Signals if the node is already running.
    #[rpc(name = "start_node")]
    fn start_node(&self) -> Result<StatusCode>;

    /// Gracefully stop the node.
    #[rpc(name = "stop_node")]
    fn stop_node(&self) -> Result<StatusCode>;

    #[rpc(name = "sign_message")]
    fn sign_message(&self, _: Vec<u8>) -> Result<(Signature, PublicKey)>;

    /// Add a new private key for the node to use to stake.
    #[rpc(name = "add_staking_keys")]
    fn add_staking_keys(&self, _: Vec<PrivateKey>) -> Result<StatusCode>;

    /// Remove an address used to stake.
    #[rpc(name = "remove_staking_keys")]
    fn remove_staking_keys(&self, _: Vec<Address>) -> Result<StatusCode>;

    /// Return hashset of staking addresses.
    #[rpc(name = "list_staking_keys")]
    fn list_staking_keys(&self) -> Result<HashSet<Address>>;

    #[rpc(name = "ban")]
    fn ban(&self, _: NodeId) -> Result<StatusCode>;

    #[rpc(name = "unban")]
    fn unban(&self, _: IpAddr) -> Result<StatusCode>;
}

pub struct API;

impl MetaMask for API {
    fn call(&self) -> Result<StatusCode> {
        todo!()
    }

    fn get_balance(&self) -> Result<Amount> {
        todo!()
    }

    fn send_transaction(&self) -> Result<StatusCode> {
        todo!()
    }
}

impl MassaPublic for API {
    fn get_status(&self) -> Result<NodeStatus> {
        todo!()
    }

    fn get_cliques(&self) -> Result<Vec<Clique>> {
        todo!()
    }

    fn get_stakers(&self) -> Result<HashMap<Address, u64>> {
        todo!()
    }

    fn get_operations(&self, _: Vec<OperationId>) -> Result<Vec<OperationInfo>> {
        todo!()
    }

    fn get_addresses(&self, _: Vec<Address>) -> Result<Vec<AddressInfo>> {
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
    fn start_node(&self) -> Result<StatusCode> {
        todo!()
    }

    fn stop_node(&self) -> Result<StatusCode> {
        todo!()
    }

    fn sign_message(&self, _: Vec<u8>) -> Result<(Signature, PublicKey)> {
        todo!()
    }

    fn add_staking_keys(&self, _: Vec<PrivateKey>) -> Result<StatusCode> {
        todo!()
    }

    fn remove_staking_keys(&self, _: Vec<Address>) -> Result<StatusCode> {
        todo!()
    }

    fn list_staking_keys(&self) -> Result<HashSet<Address>> {
        todo!()
    }

    fn ban(&self, _: NodeId) -> Result<StatusCode> {
        todo!()
    }

    fn unban(&self, _: IpAddr) -> Result<StatusCode> {
        todo!()
    }
}
