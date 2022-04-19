// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! All the structures that are used everywhere
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
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
/// active blocks related structures
pub mod active_block;
/// address related structures
pub mod address;
/// amount related structures
pub mod amount;
/// structure use by the API
pub mod api;
mod block;
/// clique
pub mod clique;
/// various structures
pub mod composite;
mod endorsement;
/// models error
pub mod error;
/// execution related structures
pub mod execution;
/// ledger related structures
pub mod ledger_models;
/// node related structure
pub mod node;
mod node_configuration;
/// operations
pub mod operation;
/// smart contract output events
pub mod output_event;
/// pre-hashed trait, for hash less hashmap/set
pub mod prehash;
/// rolls
pub mod rolls;
mod serialization;
mod serialization_context;
/// trait for signed structure
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
