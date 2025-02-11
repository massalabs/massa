// Copyright (c) 2023 MASSA LABS <info@massa.net>

use massa_proto_rs::massa::api::v1 as grpc_api;

use crate::private::{
    add_staking_secret_keys, add_to_bootstrap_blacklist, add_to_bootstrap_whitelist,
    add_to_peers_whitelist, allow_everyone_to_bootstrap, ban_nodes_by_ids, ban_nodes_by_ips,
    get_bootstrap_blacklist, get_bootstrap_whitelist, get_mip_status, get_node_status,
    get_peers_whitelist, remove_from_bootstrap_blacklist, remove_from_bootstrap_whitelist,
    remove_from_peers_whitelist, remove_staking_addresses, shutdown_gracefully, sign_messages,
    unban_nodes_by_ids, unban_nodes_by_ips,
};
use crate::public::{
    execute_read_only_call, get_blocks, get_datastore_entries, get_endorsements,
    get_next_block_best_parents, get_operations, get_sc_execution_events, get_selector_draws,
    get_stakers, get_status, get_transactions_throughput, query_state, search_blocks,
    search_endorsements, search_operations,
};

#[cfg(feature = "execution-trace")]
use crate::public::{get_operation_abi_call_stacks, get_slot_abi_call_stacks, get_slot_transfers};
use crate::stream::new_blocks::{new_blocks_server, NewBlocksServerStreamType};
use crate::stream::new_endorsements::{new_endorsements_server, NewEndorsementsServerStreamType};
use crate::stream::new_filled_blocks::{new_filled_blocks_server, NewFilledBlocksServerStreamType};
use crate::stream::new_operations::{new_operations_server, NewOperationsServerStreamType};
use crate::stream::new_slot_execution_outputs::{
    new_slot_execution_outputs_server, NewSlotExecutionOutputsServerStreamType,
};
#[cfg(feature = "execution-trace")]
use crate::stream::new_slot_transfers::new_slot_transfers;

use crate::server::{MassaPrivateGrpc, MassaPublicGrpc};
use crate::stream::tx_throughput::{
    transactions_throughput_server, TransactionsThroughputServerStreamType,
};
use crate::stream::{
    new_blocks::{new_blocks, NewBlocksStreamType},
    new_endorsements::{new_endorsements, NewEndorsementsStreamType},
    new_filled_blocks::{new_filled_blocks, NewFilledBlocksStreamType},
    new_operations::{new_operations, NewOperationsStreamType},
    new_slot_abi_call_stacks::{new_slot_abi_call_stacks, NewSlotABICallStacksStreamType},
    new_slot_execution_outputs::{new_slot_execution_outputs, NewSlotExecutionOutputsStreamType},
    new_slot_transfers::NewSlotTransfersStreamType,
    send_blocks::SendBlocksStreamType,
    send_endorsements::{send_endorsements, SendEndorsementsStreamType},
    send_operations::{send_operations, SendOperationsStreamType},
    tx_throughput::{transactions_throughput, TransactionsThroughputStreamType},
};

#[tonic::async_trait]
impl grpc_api::public_service_server::PublicService for MassaPublicGrpc {
    /// Execute read only call
    async fn execute_read_only_call(
        &self,
        request: tonic::Request<grpc_api::ExecuteReadOnlyCallRequest>,
    ) -> std::result::Result<tonic::Response<grpc_api::ExecuteReadOnlyCallResponse>, tonic::Status>
    {
        Ok(tonic::Response::new(execute_read_only_call(self, request)?))
    }

    #[cfg(feature = "execution-trace")]
    async fn get_operation_abi_call_stacks(
        &self,
        request: tonic::Request<grpc_api::GetOperationAbiCallStacksRequest>,
    ) -> std::result::Result<
        tonic::Response<grpc_api::GetOperationAbiCallStacksResponse>,
        tonic::Status,
    > {
        Ok(tonic::Response::new(get_operation_abi_call_stacks(
            self, request,
        )?))
    }

    #[cfg(not(feature = "execution-trace"))]
    async fn get_operation_abi_call_stacks(
        &self,
        _request: tonic::Request<grpc_api::GetOperationAbiCallStacksRequest>,
    ) -> std::result::Result<
        tonic::Response<grpc_api::GetOperationAbiCallStacksResponse>,
        tonic::Status,
    > {
        Err(tonic::Status::unimplemented("feature not enabled"))
    }

    #[cfg(feature = "execution-trace")]
    async fn get_slot_abi_call_stacks(
        &self,
        request: tonic::Request<grpc_api::GetSlotAbiCallStacksRequest>,
    ) -> std::result::Result<tonic::Response<grpc_api::GetSlotAbiCallStacksResponse>, tonic::Status>
    {
        Ok(tonic::Response::new(get_slot_abi_call_stacks(
            self, request,
        )?))
    }

    #[cfg(not(feature = "execution-trace"))]
    async fn get_slot_abi_call_stacks(
        &self,
        _request: tonic::Request<grpc_api::GetSlotAbiCallStacksRequest>,
    ) -> std::result::Result<tonic::Response<grpc_api::GetSlotAbiCallStacksResponse>, tonic::Status>
    {
        Err(tonic::Status::unimplemented("feature not enabled"))
    }

    #[cfg(feature = "execution-trace")]
    async fn get_slot_transfers(
        &self,
        request: tonic::Request<grpc_api::GetSlotTransfersRequest>,
    ) -> std::result::Result<tonic::Response<grpc_api::GetSlotTransfersResponse>, tonic::Status>
    {
        Ok(tonic::Response::new(get_slot_transfers(self, request)?))
    }

    #[cfg(not(feature = "execution-trace"))]
    async fn get_slot_transfers(
        &self,
        _request: tonic::Request<grpc_api::GetSlotTransfersRequest>,
    ) -> std::result::Result<tonic::Response<grpc_api::GetSlotTransfersResponse>, tonic::Status>
    {
        Err(tonic::Status::unimplemented("feature not enabled"))
    }

    /// handler for get blocks
    async fn get_blocks(
        &self,
        request: tonic::Request<grpc_api::GetBlocksRequest>,
    ) -> Result<tonic::Response<grpc_api::GetBlocksResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_blocks(self, request)?))
    }

    /// handler for get multiple datastore entries
    async fn get_datastore_entries(
        &self,
        request: tonic::Request<grpc_api::GetDatastoreEntriesRequest>,
    ) -> Result<tonic::Response<grpc_api::GetDatastoreEntriesResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_datastore_entries(self, request)?))
    }

    /// handler for get endorsements
    async fn get_endorsements(
        &self,
        request: tonic::Request<grpc_api::GetEndorsementsRequest>,
    ) -> Result<tonic::Response<grpc_api::GetEndorsementsResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_endorsements(self, request)?))
    }

    /// handler for get largest stakers
    async fn get_stakers(
        &self,
        request: tonic::Request<grpc_api::GetStakersRequest>,
    ) -> Result<tonic::Response<grpc_api::GetStakersResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_stakers(self, request)?))
    }

    /// handler for get satatus
    async fn get_status(
        &self,
        request: tonic::Request<grpc_api::GetStatusRequest>,
    ) -> Result<tonic::Response<grpc_api::GetStatusResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_status(self, request)?))
    }

    /// handler for get next block best parents
    async fn get_next_block_best_parents(
        &self,
        request: tonic::Request<grpc_api::GetNextBlockBestParentsRequest>,
    ) -> Result<tonic::Response<grpc_api::GetNextBlockBestParentsResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_next_block_best_parents(
            self, request,
        )?))
    }

    /// handler for get operations
    async fn get_operations(
        &self,
        request: tonic::Request<grpc_api::GetOperationsRequest>,
    ) -> Result<tonic::Response<grpc_api::GetOperationsResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_operations(self, request)?))
    }

    /// handler for get smart contract execution events
    async fn get_sc_execution_events(
        &self,
        request: tonic::Request<grpc_api::GetScExecutionEventsRequest>,
    ) -> Result<tonic::Response<grpc_api::GetScExecutionEventsResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_sc_execution_events(
            self, request,
        )?))
    }

    /// handler for get selector draws
    async fn get_selector_draws(
        &self,
        request: tonic::Request<grpc_api::GetSelectorDrawsRequest>,
    ) -> Result<tonic::Response<grpc_api::GetSelectorDrawsResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_selector_draws(self, request)?))
    }

    /// handler for get transactions throughput
    async fn get_transactions_throughput(
        &self,
        request: tonic::Request<grpc_api::GetTransactionsThroughputRequest>,
    ) -> Result<tonic::Response<grpc_api::GetTransactionsThroughputResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_transactions_throughput(
            self, request,
        )?))
    }

    /// handler for get version
    async fn query_state(
        &self,
        request: tonic::Request<grpc_api::QueryStateRequest>,
    ) -> Result<tonic::Response<grpc_api::QueryStateResponse>, tonic::Status> {
        Ok(tonic::Response::new(query_state(self, request)?))
    }

    /// handler for search blocks
    async fn search_blocks(
        &self,
        request: tonic::Request<grpc_api::SearchBlocksRequest>,
    ) -> Result<tonic::Response<grpc_api::SearchBlocksResponse>, tonic::Status> {
        Ok(tonic::Response::new(search_blocks(self, request)?))
    }

    /// handler for search endorsemets
    async fn search_endorsements(
        &self,
        request: tonic::Request<grpc_api::SearchEndorsementsRequest>,
    ) -> Result<tonic::Response<grpc_api::SearchEndorsementsResponse>, tonic::Status> {
        Ok(tonic::Response::new(search_endorsements(self, request)?))
    }

    /// handler for search operations
    async fn search_operations(
        &self,
        request: tonic::Request<grpc_api::SearchOperationsRequest>,
    ) -> Result<tonic::Response<grpc_api::SearchOperationsResponse>, tonic::Status> {
        Ok(tonic::Response::new(search_operations(self, request)?))
    }

    // ███████╗████████╗██████╗ ███████╗ █████╗ ███╗   ███╗
    // ██╔════╝╚══██╔══╝██╔══██╗██╔════╝██╔══██╗████╗ ████║
    // ███████╗   ██║   ██████╔╝█████╗  ███████║██╔████╔██║
    // ╚════██║   ██║   ██╔══██╗██╔══╝  ██╔══██║██║╚██╔╝██║
    // ███████║   ██║   ██║  ██║███████╗██║  ██║██║ ╚═╝ ██║

    type NewBlocksStream = NewBlocksStreamType;

    /// handler for subscribe new blocks
    async fn new_blocks(
        &self,
        request: tonic::Request<tonic::Streaming<grpc_api::NewBlocksRequest>>,
    ) -> Result<tonic::Response<Self::NewBlocksStream>, tonic::Status> {
        Ok(tonic::Response::new(new_blocks(self, request).await?))
    }

    type NewBlocksServerStream = NewBlocksServerStreamType;
    /// handler for subscribe new blocks unidirectional
    async fn new_blocks_server(
        &self,
        request: tonic::Request<grpc_api::NewBlocksServerRequest>,
    ) -> Result<tonic::Response<Self::NewBlocksServerStream>, tonic::Status> {
        Ok(tonic::Response::new(
            new_blocks_server(self, request).await?,
        ))
    }

    type NewEndorsementsStream = NewEndorsementsStreamType;

    /// handler for subscribe new endorsements stream
    async fn new_endorsements(
        &self,
        request: tonic::Request<tonic::Streaming<grpc_api::NewEndorsementsRequest>>,
    ) -> Result<tonic::Response<Self::NewEndorsementsStream>, tonic::Status> {
        Ok(tonic::Response::new(new_endorsements(self, request).await?))
    }

    type NewEndorsementsServerStream = NewEndorsementsServerStreamType;

    /// handler for subscribe new endorsements  unidirectional stream
    async fn new_endorsements_server(
        &self,
        request: tonic::Request<grpc_api::NewEndorsementsServerRequest>,
    ) -> Result<tonic::Response<Self::NewEndorsementsServerStream>, tonic::Status> {
        Ok(tonic::Response::new(
            new_endorsements_server(self, request).await?,
        ))
    }

    type NewFilledBlocksStream = NewFilledBlocksStreamType;

    /// handler for subscribe new blocks with operations content
    async fn new_filled_blocks(
        &self,
        request: tonic::Request<tonic::Streaming<grpc_api::NewFilledBlocksRequest>>,
    ) -> Result<tonic::Response<Self::NewFilledBlocksStream>, tonic::Status> {
        Ok(tonic::Response::new(
            new_filled_blocks(self, request).await?,
        ))
    }

    type NewFilledBlocksServerStream = NewFilledBlocksServerStreamType;

    async fn new_filled_blocks_server(
        &self,
        request: tonic::Request<grpc_api::NewFilledBlocksServerRequest>,
    ) -> Result<tonic::Response<Self::NewFilledBlocksServerStream>, tonic::Status> {
        Ok(tonic::Response::new(
            new_filled_blocks_server(self, request).await?,
        ))
    }

    type NewOperationsStream = NewOperationsStreamType;

    /// handler for subscribe new operations stream
    async fn new_operations(
        &self,
        request: tonic::Request<tonic::Streaming<grpc_api::NewOperationsRequest>>,
    ) -> Result<tonic::Response<Self::NewOperationsStream>, tonic::Status> {
        Ok(tonic::Response::new(new_operations(self, request).await?))
    }

    type NewOperationsServerStream = NewOperationsServerStreamType;

    /// handler for subscribe new operations stream
    async fn new_operations_server(
        &self,
        request: tonic::Request<grpc_api::NewOperationsServerRequest>,
    ) -> Result<tonic::Response<Self::NewOperationsServerStream>, tonic::Status> {
        Ok(tonic::Response::new(
            new_operations_server(self, request).await?,
        ))
    }

    type NewSlotExecutionOutputsStream = NewSlotExecutionOutputsStreamType;

    /// handler for subscribe new slot execution output stream
    async fn new_slot_execution_outputs(
        &self,
        request: tonic::Request<tonic::Streaming<grpc_api::NewSlotExecutionOutputsRequest>>,
    ) -> Result<tonic::Response<Self::NewSlotExecutionOutputsStream>, tonic::Status> {
        Ok(tonic::Response::new(
            new_slot_execution_outputs(self, request).await?,
        ))
    }

    type NewSlotExecutionOutputsServerStream = NewSlotExecutionOutputsServerStreamType;

    async fn new_slot_execution_outputs_server(
        &self,
        request: tonic::Request<grpc_api::NewSlotExecutionOutputsServerRequest>,
    ) -> Result<tonic::Response<Self::NewSlotExecutionOutputsServerStream>, tonic::Status> {
        Ok(tonic::Response::new(
            new_slot_execution_outputs_server(self, request).await?,
        ))
    }

    type NewSlotTransfersStream = NewSlotTransfersStreamType;

    #[cfg(feature = "execution-trace")]
    /// handler for subscribe new slot transfers stream
    async fn new_slot_transfers(
        &self,
        request: tonic::Request<tonic::Streaming<grpc_api::NewSlotTransfersRequest>>,
    ) -> Result<tonic::Response<Self::NewSlotTransfersStream>, tonic::Status> {
        Ok(tonic::Response::new(
            new_slot_transfers(self, request).await?,
        ))
    }

    #[cfg(not(feature = "execution-trace"))]
    /// handler for subscribe new slot transfers stream
    async fn new_slot_transfers(
        &self,
        _request: tonic::Request<tonic::Streaming<grpc_api::NewSlotTransfersRequest>>,
    ) -> Result<tonic::Response<Self::NewSlotTransfersStream>, tonic::Status> {
        Err(tonic::Status::unimplemented("feature not enabled"))
    }

    type NewSlotABICallStacksStream = NewSlotABICallStacksStreamType;

    /// handler for subscribe new slot abi call stacks stream
    async fn new_slot_abi_call_stacks(
        &self,
        request: tonic::Request<tonic::Streaming<grpc_api::NewSlotAbiCallStacksRequest>>,
    ) -> Result<tonic::Response<Self::NewSlotABICallStacksStream>, tonic::Status> {
        if cfg!(feature = "execution-trace") {
            Ok(tonic::Response::new(
                new_slot_abi_call_stacks(self, request).await?,
            ))
        } else {
            Err(tonic::Status::unimplemented("feature not enabled"))
        }
    }

    type SendBlocksStream = SendBlocksStreamType;

    /// handler for send_blocks_stream
    async fn send_blocks(
        &self,
        _request: tonic::Request<tonic::Streaming<grpc_api::SendBlocksRequest>>,
    ) -> Result<tonic::Response<Self::SendBlocksStream>, tonic::Status> {
        Err(tonic::Status::unavailable("not available"))
        // Ok(tonic::Response::new(send_blocks(self, request).await?))
    }

    type SendEndorsementsStream = SendEndorsementsStreamType;

    /// handler for send_endorsements
    async fn send_endorsements(
        &self,
        request: tonic::Request<tonic::Streaming<grpc_api::SendEndorsementsRequest>>,
    ) -> Result<tonic::Response<Self::SendEndorsementsStream>, tonic::Status> {
        Ok(tonic::Response::new(
            send_endorsements(self, request).await?,
        ))
    }

    type SendOperationsStream = SendOperationsStreamType;

    /// handler for send_operations
    async fn send_operations(
        &self,
        request: tonic::Request<tonic::Streaming<grpc_api::SendOperationsRequest>>,
    ) -> Result<tonic::Response<Self::SendOperationsStream>, tonic::Status> {
        Ok(tonic::Response::new(send_operations(self, request).await?))
    }

    type TransactionsThroughputStream = TransactionsThroughputStreamType;

    /// handler for transactions throughput
    async fn transactions_throughput(
        &self,
        request: tonic::Request<tonic::Streaming<grpc_api::TransactionsThroughputRequest>>,
    ) -> Result<tonic::Response<Self::TransactionsThroughputStream>, tonic::Status> {
        Ok(tonic::Response::new(
            transactions_throughput(self, request).await?,
        ))
    }

    type TransactionsThroughputServerStream = TransactionsThroughputServerStreamType;

    async fn transactions_throughput_server(
        &self,
        request: tonic::Request<grpc_api::TransactionsThroughputServerRequest>,
    ) -> Result<tonic::Response<Self::TransactionsThroughputServerStream>, tonic::Status> {
        Ok(tonic::Response::new(
            transactions_throughput_server(self, request).await?,
        ))
    }
}

#[tonic::async_trait]
impl grpc_api::private_service_server::PrivateService for MassaPrivateGrpc {
    /// Add IP addresses to node bootstrap blacklist
    async fn add_to_bootstrap_blacklist(
        &self,
        request: tonic::Request<grpc_api::AddToBootstrapBlacklistRequest>,
    ) -> Result<tonic::Response<grpc_api::AddToBootstrapBlacklistResponse>, tonic::Status> {
        Ok(tonic::Response::new(add_to_bootstrap_blacklist(
            self, request,
        )?))
    }
    /// Add IP addresses to node bootstrap whitelist
    async fn add_to_bootstrap_whitelist(
        &self,
        request: tonic::Request<grpc_api::AddToBootstrapWhitelistRequest>,
    ) -> Result<tonic::Response<grpc_api::AddToBootstrapWhitelistResponse>, tonic::Status> {
        Ok(tonic::Response::new(add_to_bootstrap_whitelist(
            self, request,
        )?))
    }
    /// Add IP addresses to node peers whitelist. No confirmation to expect.
    /// Note: If the ip was unknown it adds it to the known peers, otherwise it updates the peer type
    async fn add_to_peers_whitelist(
        &self,
        request: tonic::Request<grpc_api::AddToPeersWhitelistRequest>,
    ) -> Result<tonic::Response<grpc_api::AddToPeersWhitelistResponse>, tonic::Status> {
        Ok(tonic::Response::new(add_to_peers_whitelist(self, request)?))
    }
    /// Add staking secret keys to wallet
    async fn add_staking_secret_keys(
        &self,
        request: tonic::Request<grpc_api::AddStakingSecretKeysRequest>,
    ) -> Result<tonic::Response<grpc_api::AddStakingSecretKeysResponse>, tonic::Status> {
        Ok(tonic::Response::new(add_staking_secret_keys(
            self, request,
        )?))
    }
    /// Ban multiple nodes by their individual ids
    async fn ban_nodes_by_ids(
        &self,
        request: tonic::Request<grpc_api::BanNodesByIdsRequest>,
    ) -> Result<tonic::Response<grpc_api::BanNodesByIdsResponse>, tonic::Status> {
        Ok(tonic::Response::new(ban_nodes_by_ids(self, request)?))
    }

    /// Ban multiple nodes by their individual IP addresses
    async fn ban_nodes_by_ips(
        &self,
        request: tonic::Request<grpc_api::BanNodesByIpsRequest>,
    ) -> Result<tonic::Response<grpc_api::BanNodesByIpsResponse>, tonic::Status> {
        Ok(tonic::Response::new(ban_nodes_by_ips(self, request)?))
    }

    /// Get node bootstrap blacklist IP addresses
    async fn get_bootstrap_blacklist(
        &self,
        request: tonic::Request<grpc_api::GetBootstrapBlacklistRequest>,
    ) -> Result<tonic::Response<grpc_api::GetBootstrapBlacklistResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_bootstrap_blacklist(
            self, request,
        )?))
    }
    /// Get node bootstrap whitelist IP addresses
    async fn get_bootstrap_whitelist(
        &self,
        request: tonic::Request<grpc_api::GetBootstrapWhitelistRequest>,
    ) -> Result<tonic::Response<grpc_api::GetBootstrapWhitelistResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_bootstrap_whitelist(
            self, request,
        )?))
    }
    /// handler for get mip status (versioning)
    async fn get_mip_status(
        &self,
        request: tonic::Request<grpc_api::GetMipStatusRequest>,
    ) -> Result<tonic::Response<grpc_api::GetMipStatusResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_mip_status(self, request)?))
    }
    /// Allow everyone to bootstrap from the node by removing bootstrap whitelist configuration file
    async fn allow_everyone_to_bootstrap(
        &self,
        request: tonic::Request<grpc_api::AllowEveryoneToBootstrapRequest>,
    ) -> Result<tonic::Response<grpc_api::AllowEveryoneToBootstrapResponse>, tonic::Status> {
        Ok(tonic::Response::new(allow_everyone_to_bootstrap(
            self, request,
        )?))
    }
    /// Get node status
    async fn get_node_status(
        &self,
        request: tonic::Request<grpc_api::GetNodeStatusRequest>,
    ) -> Result<tonic::Response<grpc_api::GetNodeStatusResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_node_status(self, request)?))
    }
    /// Get node peers whitelist IP addresses
    async fn get_peers_whitelist(
        &self,
        request: tonic::Request<grpc_api::GetPeersWhitelistRequest>,
    ) -> Result<tonic::Response<grpc_api::GetPeersWhitelistResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_peers_whitelist(self, request)?))
    }
    /// Remove from bootstrap blacklist given IP addresses
    async fn remove_from_bootstrap_blacklist(
        &self,
        request: tonic::Request<grpc_api::RemoveFromBootstrapBlacklistRequest>,
    ) -> Result<tonic::Response<grpc_api::RemoveFromBootstrapBlacklistResponse>, tonic::Status>
    {
        Ok(tonic::Response::new(remove_from_bootstrap_blacklist(
            self, request,
        )?))
    }
    /// Remove from bootstrap whitelist given IP addresses
    async fn remove_from_bootstrap_whitelist(
        &self,
        request: tonic::Request<grpc_api::RemoveFromBootstrapWhitelistRequest>,
    ) -> Result<tonic::Response<grpc_api::RemoveFromBootstrapWhitelistResponse>, tonic::Status>
    {
        Ok(tonic::Response::new(remove_from_bootstrap_whitelist(
            self, request,
        )?))
    }
    /// Remove from peers whitelist given IP addresses
    async fn remove_from_peers_whitelist(
        &self,
        request: tonic::Request<grpc_api::RemoveFromPeersWhitelistRequest>,
    ) -> Result<tonic::Response<grpc_api::RemoveFromPeersWhitelistResponse>, tonic::Status> {
        Ok(tonic::Response::new(remove_from_peers_whitelist(
            self, request,
        )?))
    }
    /// Remove addresses from staking
    async fn remove_staking_addresses(
        &self,
        request: tonic::Request<grpc_api::RemoveStakingAddressesRequest>,
    ) -> Result<tonic::Response<grpc_api::RemoveStakingAddressesResponse>, tonic::Status> {
        Ok(tonic::Response::new(remove_staking_addresses(
            self, request,
        )?))
    }
    /// Sign messages with node's key
    async fn sign_messages(
        &self,
        request: tonic::Request<grpc_api::SignMessagesRequest>,
    ) -> Result<tonic::Response<grpc_api::SignMessagesResponse>, tonic::Status> {
        Ok(tonic::Response::new(sign_messages(self, request)?))
    }
    /// Shutdown the node gracefully
    async fn shutdown_gracefully(
        &self,
        request: tonic::Request<grpc_api::ShutdownGracefullyRequest>,
    ) -> Result<tonic::Response<grpc_api::ShutdownGracefullyResponse>, tonic::Status> {
        Ok(tonic::Response::new(shutdown_gracefully(self, request)?))
    }

    /// Unban multiple nodes by their individual ids
    async fn unban_nodes_by_ids(
        &self,
        request: tonic::Request<grpc_api::UnbanNodesByIdsRequest>,
    ) -> Result<tonic::Response<grpc_api::UnbanNodesByIdsResponse>, tonic::Status> {
        Ok(tonic::Response::new(unban_nodes_by_ids(self, request)?))
    }

    /// Unban multiple nodes by their individual IP addresses
    async fn unban_nodes_by_ips(
        &self,
        request: tonic::Request<grpc_api::UnbanNodesByIpsRequest>,
    ) -> Result<tonic::Response<grpc_api::UnbanNodesByIpsResponse>, tonic::Status> {
        Ok(tonic::Response::new(unban_nodes_by_ips(self, request)?))
    }
}
