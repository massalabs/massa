#[macro_use]
extern crate lazy_static;

mod address;
mod block;
mod composite;
mod context;
mod error;
mod operation;
mod serialization;
mod slot;

pub use address::{Address, ADDRESS_SIZE_BYTES};
pub use block::{Block, BlockHeader, BlockHeaderContent, BlockId, BLOCK_ID_SIZE_BYTES};
pub use composite::{
    AddressRollState, AddressesRollState, OperationSearchResult, OperationSearchResultBlockStatus,
    OperationSearchResultStatus,
};
pub use context::{
    get_serialization_context, init_serialization_context, with_serialization_context,
    SerializationContext,
};
pub use error::ModelsError;
pub use operation::{
    Operation, OperationContent, OperationId, OperationType, OPERATION_ID_SIZE_BYTES,
};
pub use serialization::{
    array_from_slice, u8_from_slice, DeserializeCompact, DeserializeMinBEInt, DeserializeVarInt,
    SerializeCompact, SerializeMinBEInt, SerializeVarInt,
};
pub use slot::{Slot, SLOT_KEY_SIZE};
