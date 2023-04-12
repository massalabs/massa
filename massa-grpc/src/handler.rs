// Copyright (c) 2023 MASSA LABS <info@massa.net>

use massa_proto::massa::api::v1 as grpc;

use crate::api::{
    get_blocks_by_slots, get_datastore_entries, get_largest_stakers, get_next_block_best_parents,
    get_selector_draws, get_transactions_throughput, get_version,
};
use crate::server::MassaGrpc;
use crate::stream::new_blocks::{new_blocks, NewBlocksStream};
use crate::stream::new_blocks_headers::{new_blocks_headers, NewBlocksHeadersStream};
use crate::stream::new_filled_blocks::{new_filled_blocks, NewFilledBlocksStream};
use crate::stream::new_operations::{new_operations, NewOperationsStream};
use crate::stream::tx_throughput::{transactions_throughput, TransactionsThroughputStream};
use crate::stream::{
    send_blocks::{send_blocks, SendBlocksStream},
    send_endorsements::{send_endorsements, SendEndorsementsStream},
    send_operations::{send_operations, SendOperationsStream},
};

#[tonic::async_trait]
impl grpc::massa_service_server::MassaService for MassaGrpc {
    /// handler for get multiple datastore entries.
    async fn get_datastore_entries(
        &self,
        request: tonic::Request<grpc::GetDatastoreEntriesRequest>,
    ) -> Result<tonic::Response<grpc::GetDatastoreEntriesResponse>, tonic::Status> {
        match get_datastore_entries(self, request) {
            Ok(response) => Ok(tonic::Response::new(response)),
            Err(e) => Err(e.into()),
        }
    }

    /// handler for get largest stakers.
    async fn get_largest_stakers(
        &self,
        request: tonic::Request<grpc::GetLargestStakersRequest>,
    ) -> Result<tonic::Response<grpc::GetLargestStakersResponse>, tonic::Status> {
        match get_largest_stakers(self, request) {
            Ok(response) => Ok(tonic::Response::new(response)),
            Err(e) => Err(e.into()),
        }
    }

    /// handler for get selector draws
    async fn get_selector_draws(
        &self,
        request: tonic::Request<grpc::GetSelectorDrawsRequest>,
    ) -> Result<tonic::Response<grpc::GetSelectorDrawsResponse>, tonic::Status> {
        match get_selector_draws(self, request) {
            Ok(response) => Ok(tonic::Response::new(response)),
            Err(e) => Err(e.into()),
        }
    }

    async fn get_transactions_throughput(
        &self,
        request: tonic::Request<grpc::GetTransactionsThroughputRequest>,
    ) -> Result<tonic::Response<grpc::GetTransactionsThroughputResponse>, tonic::Status> {
        match get_transactions_throughput(self, request) {
            Ok(response) => Ok(tonic::Response::new(response)),
            Err(e) => Err(e.into()),
        }
    }

    /// handler for get_next_block_best_parents
    async fn get_next_block_best_parents(
        &self,
        request: tonic::Request<grpc::GetNextBlockBestParentsRequest>,
    ) -> Result<tonic::Response<grpc::GetNextBlockBestParentsResponse>, tonic::Status> {
        match get_next_block_best_parents(self, request) {
            Ok(response) => Ok(tonic::Response::new(response)),
            Err(e) => Err(e.into()),
        }
    }

    async fn get_blocks_by_slots(
        &self,
        request: tonic::Request<grpc::GetBlocksBySlotsRequest>,
    ) -> Result<tonic::Response<grpc::GetBlocksBySlotsResponse>, tonic::Status> {
        match get_blocks_by_slots(self, request) {
            Ok(response) => Ok(tonic::Response::new(response)),
            Err(e) => Err(e.into()),
        }
    }

    /// handler for get version
    async fn get_version(
        &self,
        request: tonic::Request<grpc::GetVersionRequest>,
    ) -> Result<tonic::Response<grpc::GetVersionResponse>, tonic::Status> {
        match get_version(self, request) {
            Ok(response) => Ok(tonic::Response::new(response)),
            Err(e) => Err(e.into()),
        }
    }

    // ███████╗████████╗██████╗ ███████╗ █████╗ ███╗   ███╗
    // ██╔════╝╚══██╔══╝██╔══██╗██╔════╝██╔══██╗████╗ ████║
    // ███████╗   ██║   ██████╔╝█████╗  ███████║██╔████╔██║
    // ╚════██║   ██║   ██╔══██╗██╔══╝  ██╔══██║██║╚██╔╝██║
    // ███████║   ██║   ██║  ██║███████╗██║  ██║██║ ╚═╝ ██║

    type SendBlocksStream = SendBlocksStream;

    /// handler for send_blocks_stream
    async fn send_blocks(
        &self,
        request: tonic::Request<tonic::Streaming<grpc::SendBlocksRequest>>,
    ) -> Result<tonic::Response<Self::SendBlocksStream>, tonic::Status> {
        match send_blocks(self, request).await {
            Ok(res) => Ok(tonic::Response::new(res)),
            Err(e) => Err(e.into()),
        }
    }
    type SendEndorsementsStream = SendEndorsementsStream;
    /// handler for send_endorsements
    async fn send_endorsements(
        &self,
        request: tonic::Request<tonic::Streaming<grpc::SendEndorsementsRequest>>,
    ) -> Result<tonic::Response<Self::SendEndorsementsStream>, tonic::Status> {
        match send_endorsements(self, request).await {
            Ok(res) => Ok(tonic::Response::new(res)),
            Err(e) => Err(e.into()),
        }
    }
    type SendOperationsStream = SendOperationsStream;
    /// handler for send_operations
    async fn send_operations(
        &self,
        request: tonic::Request<tonic::Streaming<grpc::SendOperationsRequest>>,
    ) -> Result<tonic::Response<Self::SendOperationsStream>, tonic::Status> {
        match send_operations(self, request).await {
            Ok(res) => Ok(tonic::Response::new(res)),
            Err(e) => Err(e.into()),
        }
    }

    type TransactionsThroughputStream = TransactionsThroughputStream;

    /// handler for transactions throughput
    async fn transactions_throughput(
        &self,
        request: tonic::Request<tonic::Streaming<grpc::TransactionsThroughputRequest>>,
    ) -> Result<tonic::Response<Self::TransactionsThroughputStream>, tonic::Status> {
        match transactions_throughput(self, request).await {
            Ok(res) => Ok(tonic::Response::new(res)),
            Err(e) => Err(e.into()),
        }
    }

    type NewOperationsStream = NewOperationsStream;

    /// handler for subscribe new operations stream
    async fn new_operations(
        &self,
        request: tonic::Request<tonic::Streaming<grpc::NewOperationsRequest>>,
    ) -> Result<tonic::Response<Self::NewOperationsStream>, tonic::Status> {
        match new_operations(self, request).await {
            Ok(res) => Ok(tonic::Response::new(res)),
            Err(e) => Err(e.into()),
        }
    }

    type NewBlocksStream = NewBlocksStream;

    /// handler for subscribe new blocks
    async fn new_blocks(
        &self,
        request: tonic::Request<tonic::Streaming<grpc::NewBlocksRequest>>,
    ) -> Result<tonic::Response<Self::NewBlocksStream>, tonic::Status> {
        match new_blocks(self, request).await {
            Ok(res) => Ok(tonic::Response::new(res)),
            Err(e) => Err(e.into()),
        }
    }

    type NewBlocksHeadersStream = NewBlocksHeadersStream;

    /// handler for subscribe new blocks headers
    async fn new_blocks_headers(
        &self,
        request: tonic::Request<tonic::Streaming<grpc::NewBlocksHeadersRequest>>,
    ) -> Result<tonic::Response<Self::NewBlocksHeadersStream>, tonic::Status> {
        match new_blocks_headers(self, request).await {
            Ok(res) => Ok(tonic::Response::new(res)),
            Err(e) => Err(e.into()),
        }
    }

    type NewFilledBlocksStream = NewFilledBlocksStream;

    /// handler for subscribe new blocks with operations content
    async fn new_filled_blocks(
        &self,
        request: tonic::Request<tonic::Streaming<grpc::NewFilledBlocksRequest>>,
    ) -> Result<tonic::Response<Self::NewFilledBlocksStream>, tonic::Status> {
        match new_filled_blocks(self, request).await {
            Ok(res) => Ok(tonic::Response::new(res)),
            Err(e) => Err(e.into()),
        }
    }
}
