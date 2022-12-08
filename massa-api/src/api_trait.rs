//! Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Json RPC API for a massa-node
use jsonrpsee::core::RpcResult;
use jsonrpsee::proc_macros::rpc;
use massa_models::version::Version;

/// Exposed API methods
#[rpc(server)]
pub trait MassaApi {
    /// Get Massa node version.
    #[method(name = "get_version")]
    async fn get_version(&self) -> RpcResult<Version>;

    /// New produced blocks headers.
    #[subscription(
        name = "subscribe_new_blocks_headers" => "new_blocks_headers",
        unsubscribe = "unsubscribe_new_blocks_headers",
        item = BlockHeader
    )]
    fn subscribe_new_blocks_headers(&self);

    /// New produced block.
    #[subscription(
		name = "subscribe_new_blocks" => "new_blocks",
		unsubscribe = "unsubscribe_new_blocks",
		item = Block
	)]
    fn subscribe_new_blocks(&self);

    /// New produced block with operations content.
    #[subscription(
		name = "subscribe_new_filled_blocks" => "new_filled_blocks",
		unsubscribe = "unsubscribe_new_filled_blocks",
		item = FilledBlock
	)]
    fn subscribe_new_filled_blocks(&self);
}
