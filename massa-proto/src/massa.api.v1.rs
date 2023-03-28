/// When an address is drawn to create an endorsement it is selected for a specific index
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct IndexedSlot {
    /// Slot
    #[prost(message, optional, tag = "1")]
    pub slot: ::core::option::Option<Slot>,
    /// Endorsement index in the slot
    #[prost(fixed64, tag = "2")]
    pub index: u64,
}
/// A point in time where a block is expected
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Slot {
    /// Period
    #[prost(fixed64, tag = "1")]
    pub period: u64,
    /// Thread
    #[prost(fixed32, tag = "2")]
    pub thread: u32,
}
<<<<<<< testnet_21
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
=======
/// An endorsement, as sent in the network
>>>>>>> refactor: add complete doc and enhance
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Endorsement {
    /// object field
    #[prost(message, optional, tag = "1")]
    pub slot: ::core::option::Option<Slot>,
    /// string field
    #[prost(fixed32, tag = "2")]
    pub index: u32,
<<<<<<< testnet_21
    /// string field
=======
    /// Hash of endorsed block
    /// This is the parent in thread `self.slot.thread` of the block in which the endorsement is included
>>>>>>> refactor: add complete doc and enhance
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
    #[prost(fixed32, tag = "3")]
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
<<<<<<< testnet_21
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
=======
    /// Content in sharable, deserializable form. Is used in the secure verification protocols
    #[prost(bytes = "vec", tag = "2")]
    pub serialized_data: ::prost::alloc::vec::Vec<u8>,
    /// A cryptographically generated value using `serialized_data` and a public key.
    #[prost(string, tag = "3")]
    pub signature: ::prost::alloc::string::String,
    /// The public-key component used in the generation of the signature
    #[prost(string, tag = "4")]
    pub content_creator_pub_key: ::prost::alloc::string::String,
    /// Derived from the same public key used to generate the signature
    #[prost(string, tag = "5")]
    pub content_creator_address: ::prost::alloc::string::String,
    /// A secure hash of the data. See also \[massa_hash::Hash\]
    #[prost(string, tag = "6")]
>>>>>>> refactor: add complete doc and enhance
    pub id: ::prost::alloc::string::String,
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
/// The operation as sent in the network
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Operation {
    /// The fee they have decided for this operation
    #[prost(fixed64, tag = "1")]
    pub fee: u64,
    /// After `expire_period` slot the operation won't be included in a block
    #[prost(fixed64, tag = "2")]
    pub expire_period: u64,
    /// The type specific operation part
    #[prost(message, optional, tag = "3")]
    pub op: ::core::option::Option<OperationType>,
}
/// Type specific operation content
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct OperationType {
    /// Transfer coins from sender to recipient
    #[prost(message, optional, tag = "1")]
    pub transaction: ::core::option::Option<Transaction>,
    /// The sender buys `roll_count` rolls. Roll price is defined in configuration
    #[prost(message, optional, tag = "2")]
    pub roll_buy: ::core::option::Option<RollBuy>,
    /// The sender sells `roll_count` rolls. Roll price is defined in configuration
    #[prost(message, optional, tag = "3")]
    pub roll_sell: ::core::option::Option<RollSell>,
    /// Execute a smart contract
    #[prost(message, optional, tag = "4")]
    pub execut_sc: ::core::option::Option<ExecuteSc>,
    /// Calls an exported function from a stored smart contract
    #[prost(message, optional, tag = "5")]
    pub call_sc: ::core::option::Option<CallSc>,
}
/// Transfer coins from sender to recipient
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Transaction {
    /// Recipient address
    #[prost(string, tag = "1")]
    pub recipient_address: ::prost::alloc::string::String,
    /// Amount
    #[prost(fixed64, tag = "2")]
    pub amount: u64,
}
/// The sender buys `roll_count` rolls. Roll price is defined in configuration
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RollBuy {
    /// Roll count
    #[prost(fixed64, tag = "1")]
    pub roll_count: u64,
}
/// The sender sells `roll_count` rolls. Roll price is defined in configuration
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RollSell {
    /// Roll count
    #[prost(fixed64, tag = "1")]
    pub roll_count: u64,
}
/// Execute a smart contract
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ExecuteSc {
    /// Smart contract bytecode.
    #[prost(bytes = "vec", tag = "1")]
    pub data: ::prost::alloc::vec::Vec<u8>,
    /// The maximum amount of gas that the execution of the contract is allowed to cost
    #[prost(fixed64, tag = "2")]
    pub max_gas: u64,
    /// A key-value store associating a hash to arbitrary bytes
    #[prost(message, repeated, tag = "3")]
    pub datastore: ::prost::alloc::vec::Vec<BytesMapFieldEntry>,
}
/// Calls an exported function from a stored smart contract
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct CallSc {
    /// Target smart contract address
    #[prost(string, tag = "1")]
    pub target_addr: ::prost::alloc::string::String,
    /// Target function name. No function is called if empty
    #[prost(string, tag = "2")]
    pub target_func: ::prost::alloc::string::String,
    /// Parameter to pass to the target function
    #[prost(bytes = "vec", tag = "3")]
    pub param: ::prost::alloc::vec::Vec<u8>,
    /// The maximum amount of gas that the execution of the contract is allowed to cost
    #[prost(fixed64, tag = "4")]
    pub max_gas: u64,
    /// Extra coins that are spent from the caller's balance and transferred to the target
    #[prost(fixed64, tag = "5")]
    pub coins: u64,
}
/// Packages type Operation such that it can be securely sent and received in a trust-free network
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SecureShareOperation {
    /// Operation
    #[prost(message, optional, tag = "1")]
    pub content: ::core::option::Option<Operation>,
    /// Content in sharable, deserializable form. Is used in the secure verification protocols
    #[prost(bytes = "vec", tag = "2")]
    pub serialized_data: ::prost::alloc::vec::Vec<u8>,
    /// A cryptographically generated value using `serialized_data` and a public key.
    #[prost(string, tag = "3")]
    pub signature: ::prost::alloc::string::String,
    /// The public-key component used in the generation of the signature
    #[prost(string, tag = "4")]
    pub content_creator_pub_key: ::prost::alloc::string::String,
    /// Derived from the same public key used to generate the signature
    #[prost(string, tag = "5")]
    pub content_creator_address: ::prost::alloc::string::String,
    /// A secure hash of the data. See also \[massa_hash::Hash\]
    #[prost(string, tag = "6")]
    pub id: ::prost::alloc::string::String,
}
/// Operation filter type
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum OperationStreamFilterType {
    Transaction = 0,
    RollBuy = 1,
    RollSell = 2,
    ExecuteSc = 3,
    CallSc = 4,
}
impl OperationStreamFilterType {
    /// String value of the enum field names used in the ProtoBuf definition.
    ///
    /// The values are not transformed in any way and thus are considered stable
    /// (if the ProtoBuf definition does not change) and safe for programmatic use.
    pub fn as_str_name(&self) -> &'static str {
        match self {
            OperationStreamFilterType::Transaction => "TRANSACTION",
            OperationStreamFilterType::RollBuy => "ROLL_BUY",
            OperationStreamFilterType::RollSell => "ROLL_SELL",
            OperationStreamFilterType::ExecuteSc => "EXECUTE_SC",
            OperationStreamFilterType::CallSc => "CALL_SC",
        }
    }
    /// Creates an enum from field names used in the ProtoBuf definition.
    pub fn from_str_name(value: &str) -> ::core::option::Option<Self> {
        match value {
            "TRANSACTION" => Some(Self::Transaction),
            "ROLL_BUY" => Some(Self::RollBuy),
            "ROLL_SELL" => Some(Self::RollSell),
            "EXECUTE_SC" => Some(Self::ExecuteSc),
            "CALL_SC" => Some(Self::CallSc),
            _ => None,
        }
    }
}
/// Block
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Block {
    /// Signed header
    #[prost(message, optional, tag = "1")]
    pub header: ::core::option::Option<SecureShareBlockHeader>,
    /// Operations ids
    #[prost(string, repeated, tag = "2")]
    pub operations: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
}
/// Filled block
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct FilledBlock {
    /// Signed header
    #[prost(message, optional, tag = "1")]
    pub header: ::core::option::Option<SecureShareBlockHeader>,
    /// Operations
    #[prost(message, repeated, tag = "2")]
    pub operations: ::prost::alloc::vec::Vec<FilledOperationTuple>,
}
/// Block header
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BlockHeader {
    /// Slot
    #[prost(message, optional, tag = "1")]
    pub slot: ::core::option::Option<Slot>,
    /// parents
    #[prost(string, repeated, tag = "2")]
    pub parents: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
    /// All operations hash
    #[prost(string, tag = "3")]
    pub operation_merkle_root: ::prost::alloc::string::String,
    /// Signed endorsements
    #[prost(message, repeated, tag = "4")]
    pub endorsements: ::prost::alloc::vec::Vec<SecureShareEndorsement>,
}
/// Filled Operation Tuple
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct FilledOperationTuple {
    /// Operation id
    #[prost(string, tag = "1")]
    pub operation_id: ::prost::alloc::string::String,
    /// Signed operation
    #[prost(message, optional, tag = "2")]
    pub operation: ::core::option::Option<SecureShareOperation>,
}
/// Packages type Block such that it can be securely sent and received in a trust-free network
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SecureShareBlock {
    /// Block
    #[prost(message, optional, tag = "1")]
    pub content: ::core::option::Option<Block>,
    /// Content in sharable, deserializable form. Is used in the secure verification protocols
    #[prost(bytes = "vec", tag = "2")]
    pub serialized_data: ::prost::alloc::vec::Vec<u8>,
    /// A cryptographically generated value using `serialized_data` and a public key.
    #[prost(string, tag = "3")]
    pub signature: ::prost::alloc::string::String,
    /// The public-key component used in the generation of the signature
    #[prost(string, tag = "4")]
    pub content_creator_pub_key: ::prost::alloc::string::String,
    /// Derived from the same public key used to generate the signature
    #[prost(string, tag = "5")]
    pub content_creator_address: ::prost::alloc::string::String,
    /// A secure hash of the data. See also \[massa_hash::Hash\]
    #[prost(string, tag = "6")]
    pub id: ::prost::alloc::string::String,
}
/// Packages type BlockHeader such that it can be securely sent and received in a trust-free network
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SecureShareBlockHeader {
    /// BlockHeader
    #[prost(message, optional, tag = "1")]
    pub content: ::core::option::Option<BlockHeader>,
    /// Content in sharable, deserializable form. Is used in the secure verification protocols
    #[prost(bytes = "vec", tag = "2")]
    pub serialized_data: ::prost::alloc::vec::Vec<u8>,
    /// A cryptographically generated value using `serialized_data` and a public key.
    #[prost(string, tag = "3")]
    pub signature: ::prost::alloc::string::String,
    /// The public-key component used in the generation of the signature
    #[prost(string, tag = "4")]
    pub content_creator_pub_key: ::prost::alloc::string::String,
    /// Derived from the same public key used to generate the signature
    #[prost(string, tag = "5")]
    pub content_creator_address: ::prost::alloc::string::String,
    /// A secure hash of the data. See also \[massa_hash::Hash\]
    #[prost(string, tag = "6")]
    pub id: ::prost::alloc::string::String,
}
/// GetBlocksBySlotRequest holds request for GetBlocksBySlot
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetBlocksBySlotRequest {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Slots
    #[prost(message, repeated, tag = "2")]
    pub slots: ::prost::alloc::vec::Vec<Slot>,
}
/// GetBlocksBySlotResponse holds response from GetBlocksBySlot
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetBlocksBySlotResponse {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Blocks
    #[prost(message, repeated, tag = "2")]
    pub blocks: ::prost::alloc::vec::Vec<Block>,
}
/// GetDatastoreEntriesRequest holds request from GetDatastoreEntries
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetDatastoreEntriesRequest {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Queries
    #[prost(message, repeated, tag = "2")]
    pub queries: ::prost::alloc::vec::Vec<DatastoreEntriesQuery>,
}
/// DatastoreEntries Query
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct DatastoreEntriesQuery {
    /// Filter
    #[prost(message, optional, tag = "1")]
    pub filter: ::core::option::Option<DatastoreEntryFilter>,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct DatastoreEntryFilter {
    /// / Associated address of the entry
    #[prost(string, tag = "1")]
    pub address: ::prost::alloc::string::String,
    /// Datastore key
    #[prost(bytes = "vec", tag = "2")]
    pub key: ::prost::alloc::vec::Vec<u8>,
}
/// GetDatastoreEntriesResponse holds response from GetDatastoreEntries
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetDatastoreEntriesResponse {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Datastore entries
    #[prost(message, repeated, tag = "2")]
    pub entries: ::prost::alloc::vec::Vec<BytesMapFieldEntry>,
}
/// GetNextBlockBestParentsRequest holds request for GetNextBlockBestParents
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetNextBlockBestParentsRequest {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
}
/// GetNextBlockBestParentsResponse holds response from GetNextBlockBestParents
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetNextBlockBestParentsResponse {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Best parents
    #[prost(message, repeated, tag = "2")]
    pub parents: ::prost::alloc::vec::Vec<Parents>,
}
/// Best Parent Tuple
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Parents {
    /// Block id
    #[prost(string, tag = "1")]
    pub block_id: ::prost::alloc::string::String,
    /// Period
    #[prost(fixed64, tag = "2")]
    pub period: u64,
}
/// GetSelectorDrawsRequest holds request from GetSelectorDraws
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetSelectorDrawsRequest {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Queries
    #[prost(message, repeated, tag = "2")]
    pub queries: ::prost::alloc::vec::Vec<SelectorDrawsQuery>,
}
/// SelectorDraws Query
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SelectorDrawsQuery {
    /// Filter
    #[prost(message, optional, tag = "1")]
    pub filter: ::core::option::Option<SelectorDrawsFilter>,
}
/// SelectorDraws Filter
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SelectorDrawsFilter {
    /// Address
    #[prost(string, tag = "1")]
    pub address: ::prost::alloc::string::String,
}
/// GetSelectorDrawsResponse holds response from GetSelectorDraws
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetSelectorDrawsResponse {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Selector draws
    #[prost(message, repeated, tag = "2")]
    pub selector_draws: ::prost::alloc::vec::Vec<SelectorDraws>,
}
/// Selector draws
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SelectorDraws {
    /// Address
    #[prost(string, tag = "1")]
    pub address: ::prost::alloc::string::String,
    /// Next block draws
    #[prost(message, repeated, tag = "2")]
    pub next_block_draws: ::prost::alloc::vec::Vec<Slot>,
    /// Next endorsements draws
    #[prost(message, repeated, tag = "3")]
    pub next_endorsement_draws: ::prost::alloc::vec::Vec<IndexedSlot>,
}
/// GetTransactionsThroughputRequest holds request for GetTransactionsThroughput
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetTransactionsThroughputRequest {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
<<<<<<< testnet_21
    /// best parents
    #[prost(message, repeated, tag = "2")]
    pub data: ::prost::alloc::vec::Vec<BestParentTuple>,
=======
>>>>>>> refactor: add complete doc and enhance
}
/// GetTransactionsThroughputResponse holds response from GetTransactionsThroughput
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
<<<<<<< testnet_21
pub struct BestParentTuple {
    /// string value
=======
pub struct GetTransactionsThroughputResponse {
    /// Request id
>>>>>>> refactor: add complete doc and enhance
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Transactions throughput
    #[prost(fixed32, tag = "2")]
    pub throughput: u32,
}
/// GetVersionRequest holds request from GetVersion
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetVersionRequest {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
}
/// GetVersionResponse holds response from GetVersion
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetVersionResponse {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Version
    #[prost(string, tag = "2")]
    pub version: ::prost::alloc::string::String,
}
/// NewBlocksStreamRequest holds request for NewBlocksStream
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct NewBlocksStreamRequest {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
}
/// NewBlocksStreamResponse holds response from NewBlocksStream
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct NewBlocksStreamResponse {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Block
    #[prost(message, optional, tag = "2")]
    pub block: ::core::option::Option<Block>,
}
/// NewBlocksHeadersStreamRequest holds request for NewBlocksHeadersStream
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct NewBlocksHeadersStreamRequest {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
}
/// NewBlocksHeadersStreamResponse holds response from NewBlocksHeadersStream
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct NewBlocksHeadersStreamResponse {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Signed block header
    #[prost(message, optional, tag = "2")]
    pub block_header: ::core::option::Option<SecureShareBlockHeader>,
}
/// NewFilledBlocksStreamRequest holds request for NewFilledBlocksStream
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct NewFilledBlocksStreamRequest {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
}
/// NewFilledBlocksStreamResponse holds response from NewFilledBlocksStream
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct NewFilledBlocksStreamResponse {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Block with operations content
    #[prost(message, optional, tag = "2")]
    pub filled_block: ::core::option::Option<FilledBlock>,
}
/// NewOperationsStreamRequest holds request for NewOperationsStream
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct NewOperationsStreamRequest {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Filter
    #[prost(enumeration = "OperationStreamFilterType", repeated, tag = "2")]
    pub filter: ::prost::alloc::vec::Vec<i32>,
}
/// NewOperationsStreamResponse holds response from NewOperationsStream
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct NewOperationsStreamResponse {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Signed operation
    #[prost(message, optional, tag = "2")]
    pub operation: ::core::option::Option<SecureShareOperation>,
}
/// SendBlocksStreamRequest holds parameters to SendBlocks
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SendBlocksStreamRequest {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Signed block
    #[prost(message, optional, tag = "2")]
    pub block: ::core::option::Option<SecureShareBlock>,
}
/// SendBlocksStreamResponse holds response from SendBlocks
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SendBlocksStreamResponse {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Block result or a gRPC status
    #[prost(oneof = "send_blocks_stream_response::Result", tags = "2, 3")]
    pub result: ::core::option::Option<send_blocks_stream_response::Result>,
}
/// Nested message and enum types in `SendBlocksStreamResponse`.
pub mod send_blocks_stream_response {
    /// Block result or a gRPC status
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
    /// Block id
    #[prost(string, tag = "1")]
    pub block_id: ::prost::alloc::string::String,
}
/// SendEndorsementsStreamRequest holds parameters to SendEndorsements
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SendEndorsementsStreamRequest {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Signed endorsements
    #[prost(message, repeated, tag = "2")]
    pub endorsements: ::prost::alloc::vec::Vec<SecureShareEndorsement>,
}
/// SendEndorsementsStreamResponse holds response from SendEndorsements
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SendEndorsementsStreamResponse {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Endorsement result or gRPC status
    #[prost(oneof = "send_endorsements_stream_response::Message", tags = "2, 3")]
    pub message: ::core::option::Option<send_endorsements_stream_response::Message>,
}
/// Nested message and enum types in `SendEndorsementsStreamResponse`.
pub mod send_endorsements_stream_response {
    /// Endorsement result or gRPC status
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
    /// Endorsement(s) id(s)
    #[prost(string, repeated, tag = "1")]
    pub endorsements_ids: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
}
/// SendOperationsStreamRequest holds parameters to SendOperations
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SendOperationsStreamRequest {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Signed operations
    #[prost(message, repeated, tag = "2")]
    pub operations: ::prost::alloc::vec::Vec<SecureShareOperation>,
}
/// SendOperationsStreamResponse holds response from SendOperations
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SendOperationsStreamResponse {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Operation result or gRPC status
    #[prost(oneof = "send_operations_stream_response::Message", tags = "2, 3")]
    pub message: ::core::option::Option<send_operations_stream_response::Message>,
}
/// Nested message and enum types in `SendOperationsStreamResponse`.
pub mod send_operations_stream_response {
    /// Operation result or gRPC status
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
    /// Operation(s) id(s)
    #[prost(string, repeated, tag = "1")]
    pub operations_ids: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
}
/// TransactionsThroughputStreamRequest holds request for TransactionsThroughputStream
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TransactionsThroughputStreamRequest {
    /// Request id
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Optional timer interval in sec. Defaults to 10s
    #[prost(fixed64, optional, tag = "2")]
    pub interval: ::core::option::Option<u64>,
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
        /// GetBlocksBySlot
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
        /// NewBlocks
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
        /// NewBlocksHeaders
        pub async fn new_blocks_headers(
            &mut self,
            request: impl tonic::IntoStreamingRequest<
                Message = super::NewBlocksHeadersStreamRequest,
            >,
        ) -> std::result::Result<
            tonic::Response<
                tonic::codec::Streaming<super::NewBlocksHeadersStreamResponse>,
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
                "/massa.api.v1.Grpc/NewBlocksHeaders",
            );
            self.inner.streaming(request.into_streaming_request(), path, codec).await
        }
        /// NewFilledBlocks
        pub async fn new_filled_blocks(
            &mut self,
            request: impl tonic::IntoStreamingRequest<
                Message = super::NewFilledBlocksStreamRequest,
            >,
        ) -> std::result::Result<
            tonic::Response<
                tonic::codec::Streaming<super::NewFilledBlocksStreamResponse>,
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
                "/massa.api.v1.Grpc/NewFilledBlocks",
            );
            self.inner.streaming(request.into_streaming_request(), path, codec).await
        }
        /// NewOperations
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
        /// SendBlocks
        pub async fn send_blocks(
            &mut self,
            request: impl tonic::IntoStreamingRequest<
                Message = super::SendBlocksStreamRequest,
            >,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::SendBlocksStreamResponse>>,
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
                Message = super::SendEndorsementsStreamRequest,
            >,
        ) -> std::result::Result<
            tonic::Response<
                tonic::codec::Streaming<super::SendEndorsementsStreamResponse>,
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
                "/massa.api.v1.Grpc/SendEndorsements",
            );
            self.inner.streaming(request.into_streaming_request(), path, codec).await
        }
        /// SendOperations
        pub async fn send_operations(
            &mut self,
            request: impl tonic::IntoStreamingRequest<
                Message = super::SendOperationsStreamRequest,
            >,
        ) -> std::result::Result<
            tonic::Response<
                tonic::codec::Streaming<super::SendOperationsStreamResponse>,
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
                "/massa.api.v1.Grpc/SendOperations",
            );
            self.inner.streaming(request.into_streaming_request(), path, codec).await
        }
        /// TransactionsThroughput
        pub async fn transactions_throughput(
            &mut self,
            request: impl tonic::IntoStreamingRequest<
                Message = super::TransactionsThroughputStreamRequest,
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
                "/massa.api.v1.Grpc/TransactionsThroughput",
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
        /// GetBlocksBySlot
        async fn get_blocks_by_slot(
            &self,
            request: tonic::Request<super::GetBlocksBySlotRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetBlocksBySlotResponse>,
            tonic::Status,
        >;
        /// GetDatastoreEntries
        async fn get_datastore_entries(
            &self,
            request: tonic::Request<super::GetDatastoreEntriesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetDatastoreEntriesResponse>,
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
        /// GetSelectorDraws
        async fn get_selector_draws(
            &self,
            request: tonic::Request<super::GetSelectorDrawsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetSelectorDrawsResponse>,
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
        /// GetVersion
        async fn get_version(
            &self,
            request: tonic::Request<super::GetVersionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetVersionResponse>,
            tonic::Status,
        >;
        /// Server streaming response type for the NewBlocks method.
        type NewBlocksStream: futures_core::Stream<
                Item = std::result::Result<super::NewBlocksStreamResponse, tonic::Status>,
            >
            + Send
            + 'static;
        /// NewBlocks
        async fn new_blocks(
            &self,
            request: tonic::Request<tonic::Streaming<super::NewBlocksStreamRequest>>,
        ) -> std::result::Result<tonic::Response<Self::NewBlocksStream>, tonic::Status>;
        /// Server streaming response type for the NewBlocksHeaders method.
        type NewBlocksHeadersStream: futures_core::Stream<
                Item = std::result::Result<
                    super::NewBlocksHeadersStreamResponse,
                    tonic::Status,
                >,
            >
            + Send
            + 'static;
        /// NewBlocksHeaders
        async fn new_blocks_headers(
            &self,
            request: tonic::Request<
                tonic::Streaming<super::NewBlocksHeadersStreamRequest>,
            >,
        ) -> std::result::Result<
            tonic::Response<Self::NewBlocksHeadersStream>,
            tonic::Status,
        >;
        /// Server streaming response type for the NewFilledBlocks method.
        type NewFilledBlocksStream: futures_core::Stream<
                Item = std::result::Result<
                    super::NewFilledBlocksStreamResponse,
                    tonic::Status,
                >,
            >
            + Send
            + 'static;
        /// NewFilledBlocks
        async fn new_filled_blocks(
            &self,
            request: tonic::Request<
                tonic::Streaming<super::NewFilledBlocksStreamRequest>,
            >,
        ) -> std::result::Result<
            tonic::Response<Self::NewFilledBlocksStream>,
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
        /// NewOperations
        async fn new_operations(
            &self,
            request: tonic::Request<tonic::Streaming<super::NewOperationsStreamRequest>>,
        ) -> std::result::Result<
            tonic::Response<Self::NewOperationsStream>,
            tonic::Status,
        >;
        /// Server streaming response type for the SendBlocks method.
        type SendBlocksStream: futures_core::Stream<
                Item = std::result::Result<
                    super::SendBlocksStreamResponse,
                    tonic::Status,
                >,
            >
            + Send
            + 'static;
        /// SendBlocks
        async fn send_blocks(
            &self,
            request: tonic::Request<tonic::Streaming<super::SendBlocksStreamRequest>>,
        ) -> std::result::Result<tonic::Response<Self::SendBlocksStream>, tonic::Status>;
        /// Server streaming response type for the SendEndorsements method.
        type SendEndorsementsStream: futures_core::Stream<
                Item = std::result::Result<
                    super::SendEndorsementsStreamResponse,
                    tonic::Status,
                >,
            >
            + Send
            + 'static;
        /// SendEndorsements
        async fn send_endorsements(
            &self,
            request: tonic::Request<
                tonic::Streaming<super::SendEndorsementsStreamRequest>,
            >,
        ) -> std::result::Result<
            tonic::Response<Self::SendEndorsementsStream>,
            tonic::Status,
        >;
        /// Server streaming response type for the SendOperations method.
        type SendOperationsStream: futures_core::Stream<
                Item = std::result::Result<
                    super::SendOperationsStreamResponse,
                    tonic::Status,
                >,
            >
            + Send
            + 'static;
        /// SendOperations
        async fn send_operations(
            &self,
            request: tonic::Request<tonic::Streaming<super::SendOperationsStreamRequest>>,
        ) -> std::result::Result<
            tonic::Response<Self::SendOperationsStream>,
            tonic::Status,
        >;
        /// Server streaming response type for the TransactionsThroughput method.
        type TransactionsThroughputStream: futures_core::Stream<
                Item = std::result::Result<
                    super::GetTransactionsThroughputResponse,
                    tonic::Status,
                >,
            >
            + Send
            + 'static;
        /// TransactionsThroughput
        async fn transactions_throughput(
            &self,
            request: tonic::Request<
                tonic::Streaming<super::TransactionsThroughputStreamRequest>,
            >,
        ) -> std::result::Result<
            tonic::Response<Self::TransactionsThroughputStream>,
            tonic::Status,
        >;
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
                "/massa.api.v1.Grpc/NewBlocksHeaders" => {
                    #[allow(non_camel_case_types)]
                    struct NewBlocksHeadersSvc<T: Grpc>(pub Arc<T>);
                    impl<
                        T: Grpc,
                    > tonic::server::StreamingService<
                        super::NewBlocksHeadersStreamRequest,
                    > for NewBlocksHeadersSvc<T> {
                        type Response = super::NewBlocksHeadersStreamResponse;
                        type ResponseStream = T::NewBlocksHeadersStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                tonic::Streaming<super::NewBlocksHeadersStreamRequest>,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                (*inner).new_blocks_headers(request).await
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
                        let method = NewBlocksHeadersSvc(inner);
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
                "/massa.api.v1.Grpc/NewFilledBlocks" => {
                    #[allow(non_camel_case_types)]
                    struct NewFilledBlocksSvc<T: Grpc>(pub Arc<T>);
                    impl<
                        T: Grpc,
                    > tonic::server::StreamingService<
                        super::NewFilledBlocksStreamRequest,
                    > for NewFilledBlocksSvc<T> {
                        type Response = super::NewFilledBlocksStreamResponse;
                        type ResponseStream = T::NewFilledBlocksStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                tonic::Streaming<super::NewFilledBlocksStreamRequest>,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                (*inner).new_filled_blocks(request).await
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
                        let method = NewFilledBlocksSvc(inner);
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
                "/massa.api.v1.Grpc/SendBlocks" => {
                    #[allow(non_camel_case_types)]
                    struct SendBlocksSvc<T: Grpc>(pub Arc<T>);
                    impl<
                        T: Grpc,
                    > tonic::server::StreamingService<super::SendBlocksStreamRequest>
                    for SendBlocksSvc<T> {
                        type Response = super::SendBlocksStreamResponse;
                        type ResponseStream = T::SendBlocksStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                tonic::Streaming<super::SendBlocksStreamRequest>,
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
                    > tonic::server::StreamingService<
                        super::SendEndorsementsStreamRequest,
                    > for SendEndorsementsSvc<T> {
                        type Response = super::SendEndorsementsStreamResponse;
                        type ResponseStream = T::SendEndorsementsStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                tonic::Streaming<super::SendEndorsementsStreamRequest>,
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
                    > tonic::server::StreamingService<super::SendOperationsStreamRequest>
                    for SendOperationsSvc<T> {
                        type Response = super::SendOperationsStreamResponse;
                        type ResponseStream = T::SendOperationsStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                tonic::Streaming<super::SendOperationsStreamRequest>,
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
                "/massa.api.v1.Grpc/TransactionsThroughput" => {
                    #[allow(non_camel_case_types)]
                    struct TransactionsThroughputSvc<T: Grpc>(pub Arc<T>);
                    impl<
                        T: Grpc,
                    > tonic::server::StreamingService<
                        super::TransactionsThroughputStreamRequest,
                    > for TransactionsThroughputSvc<T> {
                        type Response = super::GetTransactionsThroughputResponse;
                        type ResponseStream = T::TransactionsThroughputStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                tonic::Streaming<super::TransactionsThroughputStreamRequest>,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                (*inner).transactions_throughput(request).await
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
                        let method = TransactionsThroughputSvc(inner);
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
