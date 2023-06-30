// Copyright (c) 2023 MASSA LABS <info@massa.net>

use massa_proto_rs::massa::api::v1 as grpc_api;

use crate::public::{
    get_blocks, get_datastore_entries, get_mip_status, get_next_block_best_parents, get_operations,
    get_sc_execution_events, get_selector_draws, get_stakers, get_transactions_throughput,
};
use crate::server::MassaGrpc;
use crate::stream::{
    new_blocks::{new_blocks, NewBlocksStreamType},
    new_endorsements::{new_endorsements, NewEndorsementsStreamType},
    new_filled_blocks::{new_filled_blocks, NewFilledBlocksStreamType},
    new_operations::{new_operations, NewOperationsStreamType},
    new_slot_execution_outputs::{new_slot_execution_outputs, NewSlotExecutionOutputsStreamType},
    send_blocks::{send_blocks, SendBlocksStreamType},
    send_endorsements::{send_endorsements, SendEndorsementsStreamType},
    send_operations::{send_operations, SendOperationsStreamType},
    tx_throughput::{transactions_throughput, TransactionsThroughputStreamType},
};

#[tonic::async_trait]
impl grpc_api::public_service_server::PublicService for MassaGrpc {
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
        // Ok(tonic::Response::new(get_status(self, request)?))
        unimplemented!("get_status is not implemented yet")
    }

    /// handler for get mip status (versioning)
    async fn get_mip_status(
        &self,
        request: tonic::Request<grpc_api::GetMipStatusRequest>,
    ) -> Result<tonic::Response<grpc_api::GetMipStatusResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_mip_status(self, request)?))
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
        // Ok(tonic::Response::new(query_state(self, request)?))
        unimplemented!("query_state not implemented yet")
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

    type NewEndorsementsStream = NewEndorsementsStreamType;

    /// handler for subscribe new operations stream
    async fn new_endorsements(
        &self,
        request: tonic::Request<tonic::Streaming<grpc_api::NewEndorsementsRequest>>,
    ) -> Result<tonic::Response<Self::NewEndorsementsStream>, tonic::Status> {
        Ok(tonic::Response::new(new_endorsements(self, request).await?))
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

    type NewOperationsStream = NewOperationsStreamType;

    /// handler for subscribe new operations stream
    async fn new_operations(
        &self,
        request: tonic::Request<tonic::Streaming<grpc_api::NewOperationsRequest>>,
    ) -> Result<tonic::Response<Self::NewOperationsStream>, tonic::Status> {
        Ok(tonic::Response::new(new_operations(self, request).await?))
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

    type SendBlocksStream = SendBlocksStreamType;

    /// handler for send_blocks_stream
    async fn send_blocks(
        &self,
        request: tonic::Request<tonic::Streaming<grpc_api::SendBlocksRequest>>,
    ) -> Result<tonic::Response<Self::SendBlocksStream>, tonic::Status> {
        Ok(tonic::Response::new(send_blocks(self, request).await?))
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
}
