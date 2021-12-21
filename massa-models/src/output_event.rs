use serde::{Deserialize, Serialize};

use crate::{Address, BlockId, DeserializeCompact, SerializeCompact, Slot};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// By product of a byte code execution
pub struct SCOutputEvent {
    context: ExecutionContext,
    /// json data string
    data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionContext {
    slot: Slot,
    bytes_address: Address,
    caller_address: Address,
    block: Option<BlockId>,
    // todo I don't know how to represent the call stack
}

impl SerializeCompact for ExecutionContext {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, crate::ModelsError> {
        todo!()
    }
}

impl DeserializeCompact for ExecutionContext {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), crate::ModelsError> {
        todo!()
    }
}

impl SerializeCompact for SCOutputEvent {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, crate::ModelsError> {
        todo!()
    }
}

impl DeserializeCompact for SCOutputEvent {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), crate::ModelsError> {
        todo!()
    }
}
