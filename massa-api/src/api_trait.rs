//! Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Json RPC API for a massa-node
use jsonrpsee::core::RpcResult;
use jsonrpsee::proc_macros::rpc;
use massa_api_exports::address::AddressInfo;
use massa_api_exports::block::BlockInfo;
use massa_api_exports::endorsement::EndorsementInfo;
use massa_api_exports::operation::OperationInfo;
use massa_models::address::Address;
use massa_models::block::BlockId;
use massa_models::endorsement::EndorsementId;
use massa_models::operation::OperationId;
use massa_models::version::Version;

/// Exposed API methods
#[rpc(server)]
pub trait MassaApi {
    /// Get Massa node version.
    #[method(name = "get_version")]
    fn get_version(&self) -> RpcResult<Version>;

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

    // Get AddressInfo list from ids
    #[method(name = "get_addresses")]
    fn get_addresses(&self, arg: Vec<Address>) -> RpcResult<Vec<AddressInfo>>;

    /// Get BlockInfo list from ids
    #[method(name = "get_blocks")]
    fn get_blocks(&self, ids: Vec<BlockId>) -> RpcResult<Vec<BlockInfo>>;

    /// Get EndorsementInfo list from ids
    #[method(name = "get_endorsements")]
    fn get_endorsements(&self, ids: Vec<EndorsementId>) -> RpcResult<Vec<EndorsementInfo>>;

    /// Get OperationInfo list from ids
    #[method(name = "get_operations")]
    fn get_operations(&self, ids: Vec<OperationId>) -> RpcResult<Vec<OperationInfo>>;
}
