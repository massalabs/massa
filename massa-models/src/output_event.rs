use crate::{
    hhasher::PreHashed, settings::EVENT_ID_SIZE_Bytes, Address, BlockId, DeserializeCompact,
    ModelsError, SerializeCompact, Slot,
};
use massa_hash::hash::Hash;
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, str::FromStr};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// By product of a byte code execution
pub struct SCOutputEvent {
    pub id: SCOutputEventId,
    pub read_only: bool,
    pub context: EventExecutionContext,
    /// json data string
    pub data: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SCOutputEventId(pub Hash);

impl FromStr for SCOutputEventId {
    type Err = ModelsError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(SCOutputEventId(Hash::from_str(s)?))
    }
}

impl PreHashed for SCOutputEventId {}

impl SCOutputEventId {
    /// ## Example
    /// ```rust
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_models::output_event::SCOutputEventId;
    /// # use massa_hash::hash::Hash;
    /// # use std::str::FromStr;
    /// # let hash = Hash::compute_from(&"hello world".as_bytes());
    /// let event = SCOutputEventId(hash);
    /// let bytes = event.to_bytes();
    /// # let res_event = SCOutputEventId::from_bytes(&bytes).unwrap();
    /// # assert_eq!(event, res_event);
    /// ```
    pub fn to_bytes(&self) -> [u8; EVENT_ID_SIZE_Bytes] {
        self.0.to_bytes()
    }

    /// ## Example
    /// ```rust
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_models::output_event::SCOutputEventId;
    /// # use massa_hash::hash::Hash;
    /// # use std::str::FromStr;
    /// # let hash = Hash::compute_from(&"hello world".as_bytes());
    /// let event = SCOutputEventId(hash);
    /// let bytes = event.clone().into_bytes();
    /// # let res_event = SCOutputEventId::from_bytes(&bytes).unwrap();
    /// # assert_eq!(event, res_event);
    /// ```
    pub fn into_bytes(self) -> [u8; EVENT_ID_SIZE_Bytes] {
        self.0.into_bytes()
    }

    /// ## Example
    /// ```rust
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_models::output_event::SCOutputEventId;
    /// # use massa_hash::hash::Hash;
    /// # use std::str::FromStr;
    /// # let hash = Hash::compute_from(&"hello world".as_bytes());
    /// let event = SCOutputEventId(hash);
    /// let bytes = event.to_bytes();
    /// let res_event = SCOutputEventId::from_bytes(&bytes).unwrap();
    /// # assert_eq!(event, res_event);
    /// ```
    pub fn from_bytes(data: &[u8; EVENT_ID_SIZE_Bytes]) -> Result<SCOutputEventId, ModelsError> {
        Ok(SCOutputEventId(
            Hash::from_bytes(data).map_err(|_| ModelsError::HashError)?,
        ))
    }

    /// ## Example
    /// ```rust
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_models::output_event::SCOutputEventId;
    /// # use massa_hash::hash::Hash;
    /// # use std::str::FromStr;
    /// # let hash = Hash::compute_from(&"hello world".as_bytes());
    /// let event = SCOutputEventId(hash);
    /// let ser = event.to_bs58_check();
    /// # let res_event = SCOutputEventId::from_bs58_check(&ser).unwrap();
    /// # assert_eq!(event, res_event);
    /// ```
    pub fn from_bs58_check(data: &str) -> Result<SCOutputEventId, ModelsError> {
        Ok(SCOutputEventId(
            Hash::from_bs58_check(data).map_err(|_| ModelsError::HashError)?,
        ))
    }

    /// ## Example
    /// ```rust
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_models::output_event::SCOutputEventId;
    /// # use massa_hash::hash::Hash;
    /// # use std::str::FromStr;
    /// # let hash = Hash::compute_from(&"hello world".as_bytes());
    /// let event = SCOutputEventId(hash);
    /// let ser = event.to_bs58_check();
    /// # let res_event = SCOutputEventId::from_bs58_check(&ser).unwrap();
    /// # assert_eq!(event, res_event);
    /// ```
    pub fn to_bs58_check(&self) -> String {
        self.0.to_bs58_check()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventExecutionContext {
    pub slot: Slot,
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
