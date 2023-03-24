/// commons
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Version {
    /// string field
    #[prost(string, tag = "1")]
    pub version: ::prost::alloc::string::String,
}
/// BytesMapFieldEntry
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BytesMapFieldEntry {
    /// bytes key
    #[prost(bytes = "vec", tag = "1")]
    pub key: ::prost::alloc::vec::Vec<u8>,
    /// bytes key
    #[prost(bytes = "vec", tag = "2")]
    pub value: ::prost::alloc::vec::Vec<u8>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SecureSharePayload {
    /// / The public key of the content creator
    #[prost(string, tag = "1")]
    pub creator_public_key: ::prost::alloc::string::String,
    /// / The signature of the content
    #[prost(string, tag = "2")]
    pub signature: ::prost::alloc::string::String,
    /// / The serialized version of the content
    #[prost(bytes = "vec", tag = "3")]
    pub serialized_content: ::prost::alloc::vec::Vec<u8>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct IndexedSlot {
    /// fixed64 field
    #[prost(fixed64, tag = "1")]
    pub index: u64,
    /// object field
    #[prost(message, optional, tag = "2")]
    pub slot: ::core::option::Option<Slot>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Slot {
    /// fixed64 field
    #[prost(fixed64, tag = "1")]
    pub period: u64,
    /// fixed32 field
    #[prost(fixed32, tag = "2")]
    pub thread: u32,
}
/// region Endorsement
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct EndorsementInfo {
    /// string field
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// bool field
    #[prost(bool, tag = "2")]
    pub in_pool: bool,
    /// string field
    #[prost(string, repeated, tag = "3")]
    pub in_blocks: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
    /// bool field
    #[prost(bool, tag = "4")]
    pub is_final: bool,
    /// object field
    #[prost(message, optional, tag = "5")]
    pub endorsement: ::core::option::Option<Endorsement>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Endorsement {
    /// object field
    #[prost(message, optional, tag = "1")]
    pub slot: ::core::option::Option<Slot>,
    /// string field
    #[prost(uint32, tag = "2")]
    pub index: u32,
    /// string field
    #[prost(string, tag = "3")]
    pub endorsed_block: ::prost::alloc::string::String,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct EndorsementContent {
    /// string field
    #[prost(string, tag = "1")]
    pub sender_public_key: ::prost::alloc::string::String,
    /// object field
    #[prost(message, optional, tag = "2")]
    pub slot: ::core::option::Option<Slot>,
    /// float field
    #[prost(uint32, tag = "3")]
    pub index: u32,
    /// string field
    #[prost(string, tag = "4")]
    pub endorsed_block: ::prost::alloc::string::String,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct EndorsementId {
    /// string field
    #[prost(string, tag = "1")]
    pub value: ::prost::alloc::string::String,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SecureShareEndorsement {
    /// object field
    #[prost(message, optional, tag = "1")]
    pub content: ::core::option::Option<Endorsement>,
    /// string field
    #[prost(string, tag = "2")]
    pub signature: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "3")]
    pub content_creator_pub_key: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "4")]
    pub content_creator_address: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "5")]
    pub id: ::prost::alloc::string::String,
}
/// region Operation
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct OperationInfo {
    /// string field
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// string field
    #[prost(string, repeated, tag = "2")]
    pub in_blocks: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
    /// bool field
    #[prost(bool, tag = "3")]
    pub in_pool: bool,
    /// bool field
    #[prost(bool, tag = "4")]
    pub is_final: bool,
    /// object field
    #[prost(message, optional, tag = "5")]
    pub operation: ::core::option::Option<SecureShareOperation>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Roll {
    /// int64 field
    #[prost(uint64, tag = "1")]
    pub roll_count: u64,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct OperationId {
    /// string field
    #[prost(string, tag = "1")]
    pub value: ::prost::alloc::string::String,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct OperationType {
    /// object field
    #[prost(message, optional, tag = "1")]
    pub transaction: ::core::option::Option<Transaction>,
    /// object field
    #[prost(message, optional, tag = "2")]
    pub execut_sc: ::core::option::Option<ExecuteSc>,
    /// object field
    #[prost(message, optional, tag = "3")]
    pub call_sc: ::core::option::Option<CallSc>,
    /// object field
    #[prost(message, optional, tag = "4")]
    pub roll_buy: ::core::option::Option<RollBuy>,
    /// object field
    #[prost(message, optional, tag = "5")]
    pub roll_sell: ::core::option::Option<RollSell>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Transaction {
    /// string field
    #[prost(string, tag = "1")]
    pub amount: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "2")]
    pub recipient_address: ::prost::alloc::string::String,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct CallSc {
    /// string field
    #[prost(string, tag = "1")]
    pub target_addr: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "2")]
    pub target_func: ::prost::alloc::string::String,
    /// byte vec
    #[prost(bytes = "vec", tag = "3")]
    pub param: ::prost::alloc::vec::Vec<u8>,
    /// float field
    #[prost(uint64, tag = "4")]
    pub max_gas: u64,
    /// uint64 field
    #[prost(uint64, tag = "5")]
    pub coins: u64,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ExecuteSc {
    /// data field
    #[prost(bytes = "vec", tag = "1")]
    pub data: ::prost::alloc::vec::Vec<u8>,
    /// uint64 field
    #[prost(uint64, tag = "2")]
    pub max_gas: u64,
    /// datastore
    #[prost(message, repeated, tag = "3")]
    pub datastore: ::prost::alloc::vec::Vec<BytesMapFieldEntry>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RollBuy {
    /// float field
    #[prost(uint64, tag = "1")]
    pub roll_count: u64,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RollSell {
    /// float field
    #[prost(uint64, tag = "1")]
    pub roll_count: u64,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct OperationInput {
    /// string field
    #[prost(string, tag = "1")]
    pub content_creator_pub_key: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "2")]
    pub signature: ::prost::alloc::string::String,
    /// string field
    #[prost(string, repeated, tag = "3")]
    pub serialized_content: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SecureShareOperation {
    /// object field
    #[prost(message, optional, tag = "1")]
    pub content: ::core::option::Option<OperationType>,
    /// string field
    #[prost(string, tag = "2")]
    pub signature: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "3")]
    pub content_creator_pub_key: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "4")]
    pub content_creator_address: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "5")]
    pub id: ::prost::alloc::string::String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum OperationStreamFilterType {
    Transaction = 0,
    ExecuteSc = 1,
    CallSc = 2,
    RollBuy = 3,
    RollSell = 4,
}
impl OperationStreamFilterType {
    /// String value of the enum field names used in the ProtoBuf definition.
    ///
    /// The values are not transformed in any way and thus are considered stable
    /// (if the ProtoBuf definition does not change) and safe for programmatic use.
    pub fn as_str_name(&self) -> &'static str {
        match self {
            OperationStreamFilterType::Transaction => "TRANSACTION",
            OperationStreamFilterType::ExecuteSc => "EXECUTE_SC",
            OperationStreamFilterType::CallSc => "CALL_SC",
            OperationStreamFilterType::RollBuy => "ROLL_BUY",
            OperationStreamFilterType::RollSell => "ROLL_SELL",
        }
    }
    /// Creates an enum from field names used in the ProtoBuf definition.
    pub fn from_str_name(value: &str) -> ::core::option::Option<Self> {
        match value {
            "TRANSACTION" => Some(Self::Transaction),
            "EXECUTE_SC" => Some(Self::ExecuteSc),
            "CALL_SC" => Some(Self::CallSc),
            "ROLL_BUY" => Some(Self::RollBuy),
            "ROLL_SELL" => Some(Self::RollSell),
            _ => None,
        }
    }
}
/// region Block
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Block {
    /// object field
    #[prost(message, optional, tag = "1")]
    pub header: ::core::option::Option<SecureShareBlockHeader>,
    /// object field
    #[prost(string, repeated, tag = "2")]
    pub operations: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BlockInfo {
    /// string field
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// object field field
    #[prost(message, optional, tag = "2")]
    pub content: ::core::option::Option<BlockInfoContent>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BlockInfoContent {
    /// bool field
    #[prost(bool, tag = "1")]
    pub is_final: bool,
    /// bool field
    #[prost(bool, tag = "2")]
    pub is_stale: bool,
    /// bool field
    #[prost(bool, tag = "3")]
    pub is_in_blockclique: bool,
    /// object field
    #[prost(message, optional, tag = "4")]
    pub block: ::core::option::Option<Block>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BlockHeader {
    /// object field
    #[prost(message, optional, tag = "1")]
    pub slot: ::core::option::Option<Slot>,
    /// string field
    #[prost(string, repeated, tag = "2")]
    pub parents: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
    /// string field
    #[prost(string, tag = "3")]
    pub operation_merkle_root: ::prost::alloc::string::String,
    /// object field
    #[prost(message, repeated, tag = "4")]
    pub endorsements: ::prost::alloc::vec::Vec<SecureShareEndorsement>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct FilledBlock {
    /// object field
    #[prost(message, optional, tag = "1")]
    pub header: ::core::option::Option<SecureShareBlockHeader>,
    /// object field
    #[prost(message, repeated, tag = "2")]
    pub operations: ::prost::alloc::vec::Vec<OperationInfo>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct FilledBlockInfo {
    /// string field
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// object field
    #[prost(message, optional, tag = "2")]
    pub content: ::core::option::Option<FilledBlockInfoContent>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct FilledBlockInfoContent {
    /// bool field
    #[prost(bool, tag = "1")]
    pub is_final: bool,
    /// bool field
    #[prost(bool, tag = "2")]
    pub is_stale: bool,
    /// bool field
    #[prost(bool, tag = "3")]
    pub is_in_blockclique: bool,
    /// object field
    #[prost(message, optional, tag = "4")]
    pub block: ::core::option::Option<FilledBlock>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SecureShareBlockHeader {
    /// object field
    #[prost(message, optional, tag = "1")]
    pub content: ::core::option::Option<BlockHeader>,
    /// string field
    #[prost(string, tag = "2")]
    pub signature: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "3")]
    pub content_creator_pub_key: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "4")]
    pub content_creator_address: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "5")]
    pub id: ::prost::alloc::string::String,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct NewBlocksStreamRequest {
    /// id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct NewBlocksStreamResponse {
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    #[prost(message, optional, tag = "2")]
    pub block: ::core::option::Option<Block>,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct NewOperationsStreamRequest {
    /// id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    #[prost(enumeration = "OperationStreamFilterType", repeated, tag = "2")]
    pub filter: ::prost::alloc::vec::Vec<i32>,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct NewOperationsStreamResponse {
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    #[prost(message, optional, tag = "2")]
    pub operation: ::core::option::Option<SecureShareOperation>,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetBlocksBySlotRequest {
    /// id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// slots
    #[prost(message, repeated, tag = "2")]
    pub slots: ::prost::alloc::vec::Vec<Slot>,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetBlocksBySlotResponse {
    /// request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// blocks
    #[prost(message, repeated, tag = "2")]
    pub blocks: ::prost::alloc::vec::Vec<Block>,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetTransactionsThroughputRequest {
    /// id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetTransactionsThroughputStreamRequest {
    /// id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// optional timer interval in sec
    #[prost(uint64, optional, tag = "2")]
    pub interval: ::core::option::Option<u64>,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetTransactionsThroughputResponse {
    /// id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// tx/s
    #[prost(uint32, tag = "2")]
    pub tx_s: u32,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetNextBlockBestParentsRequest {
    /// id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetNextBlockBestParentsResponse {
    /// id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// best parents
    #[prost(message, repeated, tag = "2")]
    pub data: ::prost::alloc::vec::Vec<BestParentTuple>,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BestParentTuple {
    #[prost(string, tag = "1")]
    pub block_id: ::prost::alloc::string::String,
    #[prost(uint64, tag = "2")]
    pub period: u64,
}
/// GetDatastoreEntriesRequest holds request from GetDatastoreEntries
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetDatastoreEntriesRequest {
    /// string value
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// string value
    #[prost(message, repeated, tag = "2")]
    pub queries: ::prost::alloc::vec::Vec<DatastoreEntriesQuery>,
}
/// DatastoreEntries Query
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct DatastoreEntriesQuery {
    /// object value
    #[prost(message, optional, tag = "1")]
    pub filter: ::core::option::Option<DatastoreEntryFilter>,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct DatastoreEntryFilter {
    /// / associated address of the entry
    #[prost(string, tag = "1")]
    pub address: ::prost::alloc::string::String,
    /// datastore key
    #[prost(bytes = "vec", tag = "2")]
    pub key: ::prost::alloc::vec::Vec<u8>,
}
/// GetDatastoreEntriesResponse holds response from GetDatastoreEntries
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetDatastoreEntriesResponse {
    /// string value
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// object value
    #[prost(message, repeated, tag = "2")]
    pub entries: ::prost::alloc::vec::Vec<BytesMapFieldEntry>,
}
/// GetSelectorDrawsRequest holds request from GetSelectorDraws
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetSelectorDrawsRequest {
    /// string value
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// object value
    #[prost(message, repeated, tag = "2")]
    pub queries: ::prost::alloc::vec::Vec<SelectorDrawsQuery>,
}
/// SelectorDraws Query
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SelectorDrawsQuery {
    /// object value
    #[prost(message, optional, tag = "1")]
    pub filter: ::core::option::Option<SelectorDrawsFilter>,
}
/// SelectorDraws Filter
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SelectorDrawsFilter {
    /// string value
    #[prost(string, tag = "1")]
    pub address: ::prost::alloc::string::String,
}
/// GetSelectorDrawsResponse holds response from GetSelectorDraws
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetSelectorDrawsResponse {
    /// string value
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// object value
    #[prost(message, repeated, tag = "2")]
    pub selector_draws: ::prost::alloc::vec::Vec<SelectorDraws>,
}
/// SelectorDraws
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SelectorDraws {
    /// string value
    #[prost(string, tag = "1")]
    pub address: ::prost::alloc::string::String,
    /// object value
    #[prost(message, repeated, tag = "2")]
    pub next_block_draws: ::prost::alloc::vec::Vec<Slot>,
    /// object value
    #[prost(message, repeated, tag = "3")]
    pub next_endorsement_draws: ::prost::alloc::vec::Vec<IndexedSlot>,
}
/// GetVersionRequest holds request from GetVersion
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetVersionRequest {
    /// string value
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "2")]
    pub version: ::prost::alloc::string::String,
}
/// GetVersionResponse holds response from GetVersion
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetVersionResponse {
    /// string value
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "2")]
    pub version: ::prost::alloc::string::String,
}
/// SendBlocksRequest holds parameters to SendBlocks
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SendBlocksRequest {
    /// string value
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// object field
    #[prost(message, optional, tag = "2")]
    pub block: ::core::option::Option<SecureSharePayload>,
}
/// SendBlocksResponse holds response from SendBlocks
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SendBlocksResponse {
    /// string value
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// object field
    #[prost(oneof = "send_blocks_response::Result", tags = "2, 3")]
    pub result: ::core::option::Option<send_blocks_response::Result>,
}
/// Nested message and enum types in `SendBlocksResponse`.
pub mod send_blocks_response {
    /// object field
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Result {
        #[prost(message, tag = "2")]
        Ok(super::BlockResult),
        #[prost(message, tag = "3")]
        Error(super::super::super::super::google::rpc::Status),
    }
}
/// Holds Block response
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BlockResult {
    /// string field
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
}
/// SendEndorsementsRequest holds parameters to SendEndorsements
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SendEndorsementsRequest {
    /// string field
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// object field
    #[prost(message, repeated, tag = "2")]
    pub endorsements: ::prost::alloc::vec::Vec<SecureSharePayload>,
}
/// SendEndorsementsResponse holds response from SendEndorsements
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SendEndorsementsResponse {
    /// string field
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// object field
    #[prost(oneof = "send_endorsements_response::Message", tags = "2, 3")]
    pub message: ::core::option::Option<send_endorsements_response::Message>,
}
/// Nested message and enum types in `SendEndorsementsResponse`.
pub mod send_endorsements_response {
    /// object field
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Message {
        #[prost(message, tag = "2")]
        Result(super::EndorsementResult),
        #[prost(message, tag = "3")]
        Error(super::super::super::super::google::rpc::Status),
    }
}
/// Holds Endorsement response
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct EndorsementResult {
    /// string field
    #[prost(string, repeated, tag = "1")]
    pub ids: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
}
/// SendOperationsRequest holds parameters to SendOperations
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SendOperationsRequest {
    /// string field
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// object field
    #[prost(message, repeated, tag = "2")]
    pub operations: ::prost::alloc::vec::Vec<SecureSharePayload>,
}
/// SendOperationsResponse holds response from SendOperations
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SendOperationsResponse {
    /// string field
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// object field
    #[prost(oneof = "send_operations_response::Message", tags = "2, 3")]
    pub message: ::core::option::Option<send_operations_response::Message>,
}
/// Nested message and enum types in `SendOperationsResponse`.
pub mod send_operations_response {
    /// object field
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Message {
        #[prost(message, tag = "2")]
        Result(super::OperationResult),
        #[prost(message, tag = "3")]
        Error(super::super::super::super::google::rpc::Status),
    }
}
/// Holds Operation response
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct OperationResult {
    /// string field
    #[prost(string, repeated, tag = "1")]
    pub ids: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
}
/// Generated client implementations.
pub mod grpc_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    /// Massa gRPC service
    #[derive(Debug, Clone)]
    pub struct GrpcClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl GrpcClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> GrpcClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::BoxBody>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> GrpcClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::BoxBody>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::BoxBody>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::BoxBody>,
            >>::Error: Into<StdError> + Send + Sync,
        {
            GrpcClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        /// GetDatastoreEntries
        pub async fn get_datastore_entries(
            &mut self,
            request: impl tonic::IntoRequest<super::GetDatastoreEntriesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetDatastoreEntriesResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/massa.api.v1.Grpc/GetDatastoreEntries",
            );
            self.inner.unary(request.into_request(), path, codec).await
        }
        /// GetSelectorDraws
        pub async fn get_selector_draws(
            &mut self,
            request: impl tonic::IntoRequest<super::GetSelectorDrawsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetSelectorDrawsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/massa.api.v1.Grpc/GetSelectorDraws",
            );
            self.inner.unary(request.into_request(), path, codec).await
        }
        /// GetVersion
        pub async fn get_version(
            &mut self,
            request: impl tonic::IntoRequest<super::GetVersionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetVersionResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/massa.api.v1.Grpc/GetVersion",
            );
            self.inner.unary(request.into_request(), path, codec).await
        }
        /// GetTransactionsThroughput
        pub async fn get_transactions_throughput(
            &mut self,
            request: impl tonic::IntoRequest<super::GetTransactionsThroughputRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetTransactionsThroughputResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/massa.api.v1.Grpc/GetTransactionsThroughput",
            );
            self.inner.unary(request.into_request(), path, codec).await
        }
        /// GetNextBlockBestParents
        pub async fn get_next_block_best_parents(
            &mut self,
            request: impl tonic::IntoRequest<super::GetNextBlockBestParentsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetNextBlockBestParentsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/massa.api.v1.Grpc/GetNextBlockBestParents",
            );
            self.inner.unary(request.into_request(), path, codec).await
        }
        pub async fn get_blocks_by_slot(
            &mut self,
            request: impl tonic::IntoRequest<super::GetBlocksBySlotRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetBlocksBySlotResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/massa.api.v1.Grpc/GetBlocksBySlot",
            );
            self.inner.unary(request.into_request(), path, codec).await
        }
        /// SendBlocks
        pub async fn send_blocks(
            &mut self,
            request: impl tonic::IntoStreamingRequest<Message = super::SendBlocksRequest>,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::SendBlocksResponse>>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/massa.api.v1.Grpc/SendBlocks",
            );
            self.inner.streaming(request.into_streaming_request(), path, codec).await
        }
        /// SendEndorsements
        pub async fn send_endorsements(
            &mut self,
            request: impl tonic::IntoStreamingRequest<
                Message = super::SendEndorsementsRequest,
            >,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::SendEndorsementsResponse>>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/massa.api.v1.Grpc/SendEndorsements",
            );
            self.inner.streaming(request.into_streaming_request(), path, codec).await
        }
        /// SendOperations
        pub async fn send_operations(
            &mut self,
            request: impl tonic::IntoStreamingRequest<
                Message = super::SendOperationsRequest,
            >,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::SendOperationsResponse>>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/massa.api.v1.Grpc/SendOperations",
            );
            self.inner.streaming(request.into_streaming_request(), path, codec).await
        }
        /// Subscribe GetTransactionsThroughput
        pub async fn subscribe_transactions_throughput(
            &mut self,
            request: impl tonic::IntoStreamingRequest<
                Message = super::GetTransactionsThroughputStreamRequest,
            >,
        ) -> std::result::Result<
            tonic::Response<
                tonic::codec::Streaming<super::GetTransactionsThroughputResponse>,
            >,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/massa.api.v1.Grpc/SubscribeTransactionsThroughput",
            );
            self.inner.streaming(request.into_streaming_request(), path, codec).await
        }
        pub async fn new_operations(
            &mut self,
            request: impl tonic::IntoStreamingRequest<
                Message = super::NewOperationsStreamRequest,
            >,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::NewOperationsStreamResponse>>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/massa.api.v1.Grpc/NewOperations",
            );
            self.inner.streaming(request.into_streaming_request(), path, codec).await
        }
        pub async fn new_blocks(
            &mut self,
            request: impl tonic::IntoStreamingRequest<
                Message = super::NewBlocksStreamRequest,
            >,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::NewBlocksStreamResponse>>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/massa.api.v1.Grpc/NewBlocks",
            );
            self.inner.streaming(request.into_streaming_request(), path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod grpc_server {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with GrpcServer.
    #[async_trait]
    pub trait Grpc: Send + Sync + 'static {
        /// GetDatastoreEntries
        async fn get_datastore_entries(
            &self,
            request: tonic::Request<super::GetDatastoreEntriesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetDatastoreEntriesResponse>,
            tonic::Status,
        >;
        /// GetSelectorDraws
        async fn get_selector_draws(
            &self,
            request: tonic::Request<super::GetSelectorDrawsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetSelectorDrawsResponse>,
            tonic::Status,
        >;
        /// GetVersion
        async fn get_version(
            &self,
            request: tonic::Request<super::GetVersionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetVersionResponse>,
            tonic::Status,
        >;
        /// GetTransactionsThroughput
        async fn get_transactions_throughput(
            &self,
            request: tonic::Request<super::GetTransactionsThroughputRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetTransactionsThroughputResponse>,
            tonic::Status,
        >;
        /// GetNextBlockBestParents
        async fn get_next_block_best_parents(
            &self,
            request: tonic::Request<super::GetNextBlockBestParentsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetNextBlockBestParentsResponse>,
            tonic::Status,
        >;
        async fn get_blocks_by_slot(
            &self,
            request: tonic::Request<super::GetBlocksBySlotRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetBlocksBySlotResponse>,
            tonic::Status,
        >;
        /// Server streaming response type for the SendBlocks method.
        type SendBlocksStream: futures_core::Stream<
                Item = std::result::Result<super::SendBlocksResponse, tonic::Status>,
            >
            + Send
            + 'static;
        /// SendBlocks
        async fn send_blocks(
            &self,
            request: tonic::Request<tonic::Streaming<super::SendBlocksRequest>>,
        ) -> std::result::Result<tonic::Response<Self::SendBlocksStream>, tonic::Status>;
        /// Server streaming response type for the SendEndorsements method.
        type SendEndorsementsStream: futures_core::Stream<
                Item = std::result::Result<
                    super::SendEndorsementsResponse,
                    tonic::Status,
                >,
            >
            + Send
            + 'static;
        /// SendEndorsements
        async fn send_endorsements(
            &self,
            request: tonic::Request<tonic::Streaming<super::SendEndorsementsRequest>>,
        ) -> std::result::Result<
            tonic::Response<Self::SendEndorsementsStream>,
            tonic::Status,
        >;
        /// Server streaming response type for the SendOperations method.
        type SendOperationsStream: futures_core::Stream<
                Item = std::result::Result<super::SendOperationsResponse, tonic::Status>,
            >
            + Send
            + 'static;
        /// SendOperations
        async fn send_operations(
            &self,
            request: tonic::Request<tonic::Streaming<super::SendOperationsRequest>>,
        ) -> std::result::Result<
            tonic::Response<Self::SendOperationsStream>,
            tonic::Status,
        >;
        /// Server streaming response type for the SubscribeTransactionsThroughput method.
        type SubscribeTransactionsThroughputStream: futures_core::Stream<
                Item = std::result::Result<
                    super::GetTransactionsThroughputResponse,
                    tonic::Status,
                >,
            >
            + Send
            + 'static;
        /// Subscribe GetTransactionsThroughput
        async fn subscribe_transactions_throughput(
            &self,
            request: tonic::Request<
                tonic::Streaming<super::GetTransactionsThroughputStreamRequest>,
            >,
        ) -> std::result::Result<
            tonic::Response<Self::SubscribeTransactionsThroughputStream>,
            tonic::Status,
        >;
        /// Server streaming response type for the NewOperations method.
        type NewOperationsStream: futures_core::Stream<
                Item = std::result::Result<
                    super::NewOperationsStreamResponse,
                    tonic::Status,
                >,
            >
            + Send
            + 'static;
        async fn new_operations(
            &self,
            request: tonic::Request<tonic::Streaming<super::NewOperationsStreamRequest>>,
        ) -> std::result::Result<
            tonic::Response<Self::NewOperationsStream>,
            tonic::Status,
        >;
        /// Server streaming response type for the NewBlocks method.
        type NewBlocksStream: futures_core::Stream<
                Item = std::result::Result<super::NewBlocksStreamResponse, tonic::Status>,
            >
            + Send
            + 'static;
        async fn new_blocks(
            &self,
            request: tonic::Request<tonic::Streaming<super::NewBlocksStreamRequest>>,
        ) -> std::result::Result<tonic::Response<Self::NewBlocksStream>, tonic::Status>;
    }
    /// Massa gRPC service
    #[derive(Debug)]
    pub struct GrpcServer<T: Grpc> {
        inner: _Inner<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    struct _Inner<T>(Arc<T>);
    impl<T: Grpc> GrpcServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            let inner = _Inner(inner);
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for GrpcServer<T>
    where
        T: Grpc,
        B: Body + Send + 'static,
        B::Error: Into<StdError> + Send + 'static,
    {
        type Response = http::Response<tonic::body::BoxBody>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            let inner = self.inner.clone();
            match req.uri().path() {
                "/massa.api.v1.Grpc/GetDatastoreEntries" => {
                    #[allow(non_camel_case_types)]
                    struct GetDatastoreEntriesSvc<T: Grpc>(pub Arc<T>);
                    impl<
                        T: Grpc,
                    > tonic::server::UnaryService<super::GetDatastoreEntriesRequest>
                    for GetDatastoreEntriesSvc<T> {
                        type Response = super::GetDatastoreEntriesResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetDatastoreEntriesRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                (*inner).get_datastore_entries(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = GetDatastoreEntriesSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/massa.api.v1.Grpc/GetSelectorDraws" => {
                    #[allow(non_camel_case_types)]
                    struct GetSelectorDrawsSvc<T: Grpc>(pub Arc<T>);
                    impl<
                        T: Grpc,
                    > tonic::server::UnaryService<super::GetSelectorDrawsRequest>
                    for GetSelectorDrawsSvc<T> {
                        type Response = super::GetSelectorDrawsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetSelectorDrawsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                (*inner).get_selector_draws(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = GetSelectorDrawsSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/massa.api.v1.Grpc/GetVersion" => {
                    #[allow(non_camel_case_types)]
                    struct GetVersionSvc<T: Grpc>(pub Arc<T>);
                    impl<T: Grpc> tonic::server::UnaryService<super::GetVersionRequest>
                    for GetVersionSvc<T> {
                        type Response = super::GetVersionResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetVersionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move { (*inner).get_version(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = GetVersionSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/massa.api.v1.Grpc/GetTransactionsThroughput" => {
                    #[allow(non_camel_case_types)]
                    struct GetTransactionsThroughputSvc<T: Grpc>(pub Arc<T>);
                    impl<
                        T: Grpc,
                    > tonic::server::UnaryService<
                        super::GetTransactionsThroughputRequest,
                    > for GetTransactionsThroughputSvc<T> {
                        type Response = super::GetTransactionsThroughputResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                super::GetTransactionsThroughputRequest,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                (*inner).get_transactions_throughput(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = GetTransactionsThroughputSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/massa.api.v1.Grpc/GetNextBlockBestParents" => {
                    #[allow(non_camel_case_types)]
                    struct GetNextBlockBestParentsSvc<T: Grpc>(pub Arc<T>);
                    impl<
                        T: Grpc,
                    > tonic::server::UnaryService<super::GetNextBlockBestParentsRequest>
                    for GetNextBlockBestParentsSvc<T> {
                        type Response = super::GetNextBlockBestParentsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                super::GetNextBlockBestParentsRequest,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                (*inner).get_next_block_best_parents(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = GetNextBlockBestParentsSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/massa.api.v1.Grpc/GetBlocksBySlot" => {
                    #[allow(non_camel_case_types)]
                    struct GetBlocksBySlotSvc<T: Grpc>(pub Arc<T>);
                    impl<
                        T: Grpc,
                    > tonic::server::UnaryService<super::GetBlocksBySlotRequest>
                    for GetBlocksBySlotSvc<T> {
                        type Response = super::GetBlocksBySlotResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetBlocksBySlotRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                (*inner).get_blocks_by_slot(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = GetBlocksBySlotSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/massa.api.v1.Grpc/SendBlocks" => {
                    #[allow(non_camel_case_types)]
                    struct SendBlocksSvc<T: Grpc>(pub Arc<T>);
                    impl<
                        T: Grpc,
                    > tonic::server::StreamingService<super::SendBlocksRequest>
                    for SendBlocksSvc<T> {
                        type Response = super::SendBlocksResponse;
                        type ResponseStream = T::SendBlocksStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                tonic::Streaming<super::SendBlocksRequest>,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move { (*inner).send_blocks(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = SendBlocksSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/massa.api.v1.Grpc/SendEndorsements" => {
                    #[allow(non_camel_case_types)]
                    struct SendEndorsementsSvc<T: Grpc>(pub Arc<T>);
                    impl<
                        T: Grpc,
                    > tonic::server::StreamingService<super::SendEndorsementsRequest>
                    for SendEndorsementsSvc<T> {
                        type Response = super::SendEndorsementsResponse;
                        type ResponseStream = T::SendEndorsementsStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                tonic::Streaming<super::SendEndorsementsRequest>,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                (*inner).send_endorsements(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = SendEndorsementsSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/massa.api.v1.Grpc/SendOperations" => {
                    #[allow(non_camel_case_types)]
                    struct SendOperationsSvc<T: Grpc>(pub Arc<T>);
                    impl<
                        T: Grpc,
                    > tonic::server::StreamingService<super::SendOperationsRequest>
                    for SendOperationsSvc<T> {
                        type Response = super::SendOperationsResponse;
                        type ResponseStream = T::SendOperationsStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                tonic::Streaming<super::SendOperationsRequest>,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                (*inner).send_operations(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = SendOperationsSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/massa.api.v1.Grpc/SubscribeTransactionsThroughput" => {
                    #[allow(non_camel_case_types)]
                    struct SubscribeTransactionsThroughputSvc<T: Grpc>(pub Arc<T>);
                    impl<
                        T: Grpc,
                    > tonic::server::StreamingService<
                        super::GetTransactionsThroughputStreamRequest,
                    > for SubscribeTransactionsThroughputSvc<T> {
                        type Response = super::GetTransactionsThroughputResponse;
                        type ResponseStream = T::SubscribeTransactionsThroughputStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                tonic::Streaming<
                                    super::GetTransactionsThroughputStreamRequest,
                                >,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                (*inner).subscribe_transactions_throughput(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = SubscribeTransactionsThroughputSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/massa.api.v1.Grpc/NewOperations" => {
                    #[allow(non_camel_case_types)]
                    struct NewOperationsSvc<T: Grpc>(pub Arc<T>);
                    impl<
                        T: Grpc,
                    > tonic::server::StreamingService<super::NewOperationsStreamRequest>
                    for NewOperationsSvc<T> {
                        type Response = super::NewOperationsStreamResponse;
                        type ResponseStream = T::NewOperationsStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                tonic::Streaming<super::NewOperationsStreamRequest>,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                (*inner).new_operations(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = NewOperationsSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/massa.api.v1.Grpc/NewBlocks" => {
                    #[allow(non_camel_case_types)]
                    struct NewBlocksSvc<T: Grpc>(pub Arc<T>);
                    impl<
                        T: Grpc,
                    > tonic::server::StreamingService<super::NewBlocksStreamRequest>
                    for NewBlocksSvc<T> {
                        type Response = super::NewBlocksStreamResponse;
                        type ResponseStream = T::NewBlocksStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                tonic::Streaming<super::NewBlocksStreamRequest>,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move { (*inner).new_blocks(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = NewBlocksSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        Ok(
                            http::Response::builder()
                                .status(200)
                                .header("grpc-status", "12")
                                .header("content-type", "application/grpc")
                                .body(empty_body())
                                .unwrap(),
                        )
                    })
                }
            }
        }
    }
    impl<T: Grpc> Clone for GrpcServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    impl<T: Grpc> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(Arc::clone(&self.0))
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: Grpc> tonic::server::NamedService for GrpcServer<T> {
        const NAME: &'static str = "massa.api.v1.Grpc";
    }
}
