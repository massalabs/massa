// Copyright (c) 2022 MASSA LABS <info@massa.net>

#![warn(missing_docs)]
#[macro_use]
extern crate lazy_static;

pub use address::Address;
pub use amount::Amount;
pub use block::{Block, BlockHeader, BlockId, SignedHeader};
pub use composite::{
    OperationSearchResult, OperationSearchResultBlockStatus, OperationSearchResultStatus,
    StakersCycleProductionStats,
};
pub use endorsement::{Endorsement, EndorsementId, SignedEndorsement};
pub use error::ModelsError;
pub use operation::{Operation, OperationId, OperationType, SignedOperation};
pub use serialization::{
    array_from_slice, u8_from_slice, DeserializeCompact, DeserializeMinBEInt, DeserializeVarInt,
    SerializeCompact, SerializeMinBEInt, SerializeVarInt,
};
pub use serialization_context::{
    get_serialization_context, init_serialization_context, with_serialization_context,
    SerializationContext,
};
pub use slot::Slot;
pub use version::Version;
pub mod active_block;
pub mod address;
pub mod amount;
/// struct use by the api
pub mod api;
mod block;
/// clique
pub mod clique;
pub mod composite;
mod endorsement;
/// models error
pub mod error;
/// execution related structs
pub mod execution;
/// ledger related structs
pub mod ledger_models;
pub mod node;
mod node_configuration;
/// operations
pub mod operation;
/// smart contract output events
pub mod output_event;
/// prehashed trait, for hash less hashmap/set
pub mod prehash;
/// rolls
pub mod rolls;
mod serialization;
mod serialization_context;
/// trait for signed struct
pub mod signed;
/// slots
pub mod slot;
/// various statistics
pub mod stats;
/// management of the relation between time and slots
pub mod timeslots;
mod version;
pub use node_configuration::CompactConfig;
/// Expose constants
pub mod constants {
    pub use crate::node_configuration::*;
}
