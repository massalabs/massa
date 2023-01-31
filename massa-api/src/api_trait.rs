//! Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Json RPC API for a massa-node
use jsonrpsee::core::RpcResult;
use jsonrpsee::proc_macros::rpc;
use massa_api_exports::page::PagedVecV2;
use massa_api_exports::ApiRequest;
use massa_models::address::Address;
use massa_models::block_id::BlockId;
use massa_models::version::Version;

/// Exposed API methods
#[rpc(server)]
pub trait MassaApi {
    /// Get the active stakers and their active roll counts for the current cycle sorted by largest roll counts.
    #[method(name = "get_largest_stakers")]
    async fn get_largest_stakers(
        &self,
        page_request: Option<ApiRequest>,
    ) -> RpcResult<PagedVecV2<(Address, u64)>>;

    /// Get the ids of best parents for the next block to be produced along with their period
    #[method(name = "get_next_block_best_parents")]
    async fn get_next_block_best_parents(&self) -> RpcResult<Vec<(BlockId, u64)>>;

    /// Get Massa node version.
    #[method(name = "get_version")]
    async fn get_version(&self) -> RpcResult<Version>;

    /// New produced block.
    #[subscription(
		name = "subscribe_new_blocks" => "new_blocks",
		unsubscribe = "unsubscribe_new_blocks",
		item = Block
	)]
    fn subscribe_new_blocks(&self);

    /// New produced blocks headers.
    #[subscription(
        name = "subscribe_new_blocks_headers" => "new_blocks_headers",
        unsubscribe = "unsubscribe_new_blocks_headers",
        item = BlockHeader
    )]
    fn subscribe_new_blocks_headers(&self);

    /// New produced blocks with operations content.
    #[subscription(
		name = "subscribe_new_filled_blocks" => "new_filled_blocks",
		unsubscribe = "unsubscribe_new_filled_blocks",
		item = FilledBlock
	)]
    fn subscribe_new_filled_blocks(&self);

    /// New produced operations.
    #[subscription(
		name = "subscribe_new_operations" => "new_operations",
		unsubscribe = "unsubscribe_new_operations",
		item = Operation
	)]
    fn subscribe_new_operations(&self);
}
