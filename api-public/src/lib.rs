// Copyright (c) 2021 MASSA LABS <info@massa.net>

use api_dto::{BlockInfo, BlockSummary, EndorsementInfo, NodeStatus, OperationInfo, RollsInfo};
use jsonrpc_core::IoHandler;
use jsonrpc_derive::rpc;
use jsonrpc_http_server::ServerBuilder;
use models::address::AddressHashMap;
use models::block::BlockId;
use models::clique::Clique;
use models::endorsement::EndorsementId;
use models::operation::{Operation, OperationId};
use rpc_server::rpc_server;
pub use rpc_server::API;
use std::thread;
use time::UTime;

/// Public Massa JSON-RPC endpoints
#[rpc(server)]
pub trait MassaPublic {
    fn serve_massa_public(&self);

    /////////////////////////////////
    // Explorer (aggregated stats) //
    /////////////////////////////////

    /// summary of the current state: time, last final blocks (hash, thread, slot, timestamp), clique count, connected nodes count
    #[rpc(name = "get_status")]
    fn get_status(&self) -> jsonrpc_core::Result<NodeStatus>;

    #[rpc(name = "get_cliques")]
    fn get_cliques(&self) -> jsonrpc_core::Result<Vec<Clique>>;

    //////////////////////////////////
    // Debug (specific information) //
    //////////////////////////////////

    /// Returns the active stakers and their roll counts for the current cycle.
    #[rpc(name = "get_stakers")]
    fn get_stakers(&self) -> jsonrpc_core::Result<AddressHashMap<RollsInfo>>;

    /// Returns operations information associated to a given list of operations' IDs.
    #[rpc(name = "get_operations")]
    fn get_operations(&self, _: Vec<OperationId>) -> jsonrpc_core::Result<Vec<OperationInfo>>;

    #[rpc(name = "get_endorsements")]
    fn get_endorsements(&self, _: Vec<EndorsementId>)
        -> jsonrpc_core::Result<Vec<EndorsementInfo>>;

    /// Get information on a block given its hash
    #[rpc(name = "get_block")]
    fn get_block(&self, _: BlockId) -> jsonrpc_core::Result<BlockInfo>;

    /// Get the block graph within the specified time interval.
    /// Optional parameters: from <time_start> (included) and to <time_end> (excluded) millisecond timestamp
    #[rpc(name = "get_graph_interval")]
    fn get_graph_interval(
        &self,
        time_start: Option<UTime>,
        time_end: Option<UTime>,
    ) -> jsonrpc_core::Result<Vec<BlockSummary>>;

    //////////////////////////////////////
    // User (interaction with the node) //
    //////////////////////////////////////

    /// Return list of all those that were sent
    #[rpc(name = "send_operations")]
    fn send_operations(&self, _: Vec<Operation>) -> jsonrpc_core::Result<Vec<OperationId>>;
}

impl MassaPublic for API {
    fn serve_massa_public(&self) {
        rpc_server!(self.clone());
    }

    fn get_status(&self) -> jsonrpc_core::Result<NodeStatus> {
        todo!()
    }

    fn get_cliques(&self) -> jsonrpc_core::Result<Vec<Clique>> {
        todo!()
    }

    fn get_stakers(&self) -> jsonrpc_core::Result<AddressHashMap<RollsInfo>> {
        todo!()
    }

    fn get_operations(&self, _: Vec<OperationId>) -> jsonrpc_core::Result<Vec<OperationInfo>> {
        todo!()
    }

    fn get_endorsements(
        &self,
        _: Vec<EndorsementId>,
    ) -> jsonrpc_core::Result<Vec<EndorsementInfo>> {
        todo!()
    }

    fn get_block(&self, _: BlockId) -> jsonrpc_core::Result<BlockInfo> {
        todo!()
    }

    fn get_graph_interval(
        &self,
        _time_start: Option<UTime>,
        _time_end: Option<UTime>,
    ) -> jsonrpc_core::Result<Vec<BlockSummary>> {
        todo!()
    }

    fn send_operations(&self, _: Vec<Operation>) -> jsonrpc_core::Result<Vec<OperationId>> {
        todo!()
    }
}
