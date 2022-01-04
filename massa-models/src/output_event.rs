use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::{Address, BlockId, DeserializeCompact, SerializeCompact, Slot};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// By product of a byte code execution
pub struct SCOutputEvent {
    pub context: EventExecutionContext,
    /// json data string
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventExecutionContext {
    pub slot: Slot,
    pub bytes_address: Address,
    pub caller_address: Address,
    pub block: Option<BlockId>,
    pub call_stack: VecDeque<Address>,
}

impl SerializeCompact for EventExecutionContext {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, crate::ModelsError> {
        todo!()
    }
}

impl DeserializeCompact for EventExecutionContext {
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
