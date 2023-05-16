// Copyright (c) 2023 MASSA LABS <info@massa.net>

use massa_proto::massa::api::v1 as grpc;

use crate::api::{
    get_blocks, get_blocks_by_slots, get_datastore_entries, get_largest_stakers,
    get_next_block_best_parents, get_operations, get_sc_execution_events, get_selector_draws,
    get_transactions_throughput, get_version, get_versioning_status,
};
use crate::server::MassaGrpc;
use crate::stream::{
    new_blocks::{new_blocks, NewBlocksStreamType},
    new_blocks_headers::{new_blocks_headers, NewBlocksHeadersStreamType},
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
impl grpc::massa_service_server::MassaService for MassaGrpc {
    /// handler for get blocks
    async fn get_blocks(
        &self,
        request: tonic::Request<grpc::GetBlocksRequest>,
    ) -> Result<tonic::Response<grpc::GetBlocksResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_blocks(self, request)?))
    }

    /// handler for get blocks by slots
    async fn get_blocks_by_slots(
        &self,
        request: tonic::Request<grpc::GetBlocksBySlotsRequest>,
    ) -> Result<tonic::Response<grpc::GetBlocksBySlotsResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_blocks_by_slots(self, request)?))
    }

    /// handler for get multiple datastore entries
    async fn get_datastore_entries(
        &self,
        request: tonic::Request<grpc::GetDatastoreEntriesRequest>,
    ) -> Result<tonic::Response<grpc::GetDatastoreEntriesResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_datastore_entries(self, request)?))
    }

    /// handler for get largest stakers
    async fn get_largest_stakers(
        &self,
        request: tonic::Request<grpc::GetLargestStakersRequest>,
    ) -> Result<tonic::Response<grpc::GetLargestStakersResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_largest_stakers(self, request)?))
    }

    /// handler for get next block best parents
    async fn get_next_block_best_parents(
        &self,
        request: tonic::Request<grpc::GetNextBlockBestParentsRequest>,
    ) -> Result<tonic::Response<grpc::GetNextBlockBestParentsResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_next_block_best_parents(
            self, request,
        )?))
    }

    /// handler for get operations
    async fn get_operations(
        &self,
        request: tonic::Request<grpc::GetOperationsRequest>,
    ) -> Result<tonic::Response<grpc::GetOperationsResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_operations(self, request)?))
    }

    /// handler for get smart contract execution events
    async fn get_sc_execution_events(
        &self,
        request: tonic::Request<grpc::GetScExecutionEventsRequest>,
    ) -> Result<tonic::Response<grpc::GetScExecutionEventsResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_sc_execution_events(
            self, request,
        )?))
    }

    /// handler for get selector draws
    async fn get_selector_draws(
        &self,
        request: tonic::Request<grpc::GetSelectorDrawsRequest>,
    ) -> Result<tonic::Response<grpc::GetSelectorDrawsResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_selector_draws(self, request)?))
    }

    /// handler for get transactions throughput
    async fn get_transactions_throughput(
        &self,
        request: tonic::Request<grpc::GetTransactionsThroughputRequest>,
    ) -> Result<tonic::Response<grpc::GetTransactionsThroughputResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_transactions_throughput(
            self, request,
        )?))
    }

    /// handler for get version
    async fn get_version(
        &self,
        request: tonic::Request<grpc::GetVersionRequest>,
    ) -> Result<tonic::Response<grpc::GetVersionResponse>, tonic::Status> {
        Ok(tonic::Response::new(get_version(self, request)?))
    }

    /// TODO
    async fn get_versioning_status(
        &self,
        request: tonic::Request<grpc::GetVersioningStatusRequest>,
    ) -> Result<tonic::Response<grpc::GetVersioningStatusResponse>, tonic::Status> {
        println!("[{}][{}] get_versioning_status call", file!(), line!());
        Ok(tonic::Response::new(get_versioning_status(self, request)?))
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
        request: tonic::Request<tonic::Streaming<grpc::NewBlocksRequest>>,
    ) -> Result<tonic::Response<Self::NewBlocksStream>, tonic::Status> {
        Ok(tonic::Response::new(new_blocks(self, request).await?))
    }

    type NewBlocksHeadersStream = NewBlocksHeadersStreamType;

    /// handler for subscribe new blocks headers
    async fn new_blocks_headers(
        &self,
        request: tonic::Request<tonic::Streaming<grpc::NewBlocksHeadersRequest>>,
    ) -> Result<tonic::Response<Self::NewBlocksHeadersStream>, tonic::Status> {
        Ok(tonic::Response::new(
            new_blocks_headers(self, request).await?,
        ))
    }

    type NewEndorsementsStream = NewEndorsementsStreamType;

    /// handler for subscribe new operations stream
    async fn new_endorsements(
        &self,
        request: tonic::Request<tonic::Streaming<grpc::NewEndorsementsRequest>>,
    ) -> Result<tonic::Response<Self::NewEndorsementsStream>, tonic::Status> {
        Ok(tonic::Response::new(new_endorsements(self, request).await?))
    }

    type NewFilledBlocksStream = NewFilledBlocksStreamType;

    /// handler for subscribe new blocks with operations content
    async fn new_filled_blocks(
        &self,
        request: tonic::Request<tonic::Streaming<grpc::NewFilledBlocksRequest>>,
    ) -> Result<tonic::Response<Self::NewFilledBlocksStream>, tonic::Status> {
        Ok(tonic::Response::new(
            new_filled_blocks(self, request).await?,
        ))
    }

    type NewOperationsStream = NewOperationsStreamType;

    /// handler for subscribe new operations stream
    async fn new_operations(
        &self,
        request: tonic::Request<tonic::Streaming<grpc::NewOperationsRequest>>,
    ) -> Result<tonic::Response<Self::NewOperationsStream>, tonic::Status> {
        Ok(tonic::Response::new(new_operations(self, request).await?))
    }

    type NewSlotExecutionOutputsStream = NewSlotExecutionOutputsStreamType;

    /// handler for subscribe new slot execution output stream
    async fn new_slot_execution_outputs(
        &self,
        request: tonic::Request<tonic::Streaming<grpc::NewSlotExecutionOutputsRequest>>,
    ) -> Result<tonic::Response<Self::NewSlotExecutionOutputsStream>, tonic::Status> {
        Ok(tonic::Response::new(
            new_slot_execution_outputs(self, request).await?,
        ))
    }

    type SendBlocksStream = SendBlocksStreamType;

    /// handler for send_blocks_stream
    async fn send_blocks(
        &self,
        request: tonic::Request<tonic::Streaming<grpc::SendBlocksRequest>>,
    ) -> Result<tonic::Response<Self::SendBlocksStream>, tonic::Status> {
        Ok(tonic::Response::new(send_blocks(self, request).await?))
    }

    type SendEndorsementsStream = SendEndorsementsStreamType;

    /// handler for send_endorsements
    async fn send_endorsements(
        &self,
        request: tonic::Request<tonic::Streaming<grpc::SendEndorsementsRequest>>,
    ) -> Result<tonic::Response<Self::SendEndorsementsStream>, tonic::Status> {
        Ok(tonic::Response::new(
            send_endorsements(self, request).await?,
        ))
    }

    type SendOperationsStream = SendOperationsStreamType;

    /// handler for send_operations
    async fn send_operations(
        &self,
        request: tonic::Request<tonic::Streaming<grpc::SendOperationsRequest>>,
    ) -> Result<tonic::Response<Self::SendOperationsStream>, tonic::Status> {
        Ok(tonic::Response::new(send_operations(self, request).await?))
    }

    type TransactionsThroughputStream = TransactionsThroughputStreamType;

    /// handler for transactions throughput
    async fn transactions_throughput(
        &self,
        request: tonic::Request<tonic::Streaming<grpc::TransactionsThroughputRequest>>,
    ) -> Result<tonic::Response<Self::TransactionsThroughputStream>, tonic::Status> {
        Ok(tonic::Response::new(
            transactions_throughput(self, request).await?,
        ))
    }
}
