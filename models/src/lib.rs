// Copyright (c) 2021 MASSA LABS <info@massa.net>

#[macro_use]
extern crate lazy_static;

pub use address::{Address, ADDRESS_SIZE_BYTES};
pub use amount::Amount;
pub use block::{Block, BlockHeader, BlockHeaderContent, BlockId, BLOCK_ID_SIZE_BYTES};
pub use composite::{
    OperationSearchResult, OperationSearchResultBlockStatus, OperationSearchResultStatus,
    StakersCycleProductionStats,
};
pub use context::{
    get_serialization_context, init_serialization_context, with_serialization_context,
    SerializationContext,
};
use crypto::signature::{PublicKey, Signature};
pub use endorsement::{Endorsement, EndorsementContent, EndorsementId};
pub use error::ModelsError;
pub use operation::{
    Operation, OperationContent, OperationId, OperationType, OPERATION_ID_SIZE_BYTES,
};
use serde::{Deserialize, Serialize};
pub use serialization::{
    array_from_slice, u8_from_slice, DeserializeCompact, DeserializeMinBEInt, DeserializeVarInt,
    SerializeCompact, SerializeMinBEInt, SerializeVarInt,
};
pub use slot::{Slot, SLOT_KEY_SIZE};
pub use version::Version;

pub mod address;
pub mod amount;
mod block;
mod composite;
mod context;
mod endorsement;
pub mod error;
pub mod ledger;
pub mod operation;
mod serialization;
pub mod slot;
mod version;

#[derive(Debug, Serialize, Deserialize)]
pub struct PubkeySig {
    pub public_key: PublicKey,
    pub signature: Signature,
}
