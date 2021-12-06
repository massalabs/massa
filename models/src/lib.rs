// Copyright (c) 2021 MASSA LABS <info@massa.net>

#[macro_use]
extern crate lazy_static;
pub use address::{Address, ADDRESS_SIZE_BYTES};
pub use amount::Amount;
pub use block::{
    Block, BlockHashMap, BlockHashSet, BlockHeader, BlockHeaderContent, BlockId,
    BLOCK_ID_SIZE_BYTES,
};
pub use composite::{
    OperationSearchResult, OperationSearchResultBlockStatus, OperationSearchResultStatus,
    StakersCycleProductionStats,
};
pub use config::AlgoConfig;
pub use context::{
    get_serialization_context, init_serialization_context, with_serialization_context,
    SerializationContext,
};
pub use endorsement::{
    Endorsement, EndorsementContent, EndorsementHashMap, EndorsementHashSet, EndorsementId,
};
pub use error::ModelsError;
pub use operation::{
    Operation, OperationContent, OperationHashMap, OperationHashSet, OperationId, OperationType,
    OPERATION_ID_SIZE_BYTES,
};
pub use serialization::{
    array_from_slice, u8_from_slice, DeserializeCompact, DeserializeMinBEInt, DeserializeVarInt,
    SerializeCompact, SerializeMinBEInt, SerializeVarInt,
};
pub use slot::{Slot, SLOT_KEY_SIZE};
pub use version::Version;

pub mod address;
pub mod amount;
pub mod api;
mod block;
pub mod clique;
mod composite;
mod config;
mod context;
mod endorsement;
pub mod error;
pub mod hhasher;
pub mod ledger;
pub mod massa_hash;
pub mod node;
pub mod operation;
mod serialization;
pub mod slot;
pub mod stats;
pub mod timeslots;
mod version;
