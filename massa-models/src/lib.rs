// Copyright (c) 2022 MASSA LABS <info@massa.net>

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
pub mod api;
mod block;
pub mod clique;
pub mod composite;
mod endorsement;
pub mod error;
pub mod execution;
pub mod ledger_models;
pub mod node;
mod node_configuration;
pub mod operation;
pub mod output_event;
pub mod prehash;
pub mod rolls;
mod serialization;
mod serialization_context;
pub mod signed;
pub mod slot;
pub mod stats;
pub mod timeslots;
mod version;
pub use node_configuration::CompactConfig;
/// Expose constants
pub mod constants {
    pub use crate::node_configuration::*;
}
