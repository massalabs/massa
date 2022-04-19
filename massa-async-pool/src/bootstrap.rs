//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Provides serializable structures for bootstrapping the `AsyncPool`

use crate::message::AsyncMessage;
use massa_models::{
    DeserializeCompact, DeserializeVarInt, ModelsError, SerializeCompact, SerializeVarInt,
};
use serde::{Deserialize, Serialize};

/// Represents a snapshot of the asynchronous pool aimed at bootstrapping an `AsyncPool`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsyncPoolBootstrap {
    /// list of asynchronous messages in no particular order
    pub(crate) messages: Vec<AsyncMessage>,
}

impl SerializeCompact for AsyncPoolBootstrap {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, massa_models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // message count
        let message_count: u64 = self.messages.len().try_into().map_err(|_| {
            ModelsError::SerializeError("could not represent message count as u64".into())
        })?;
        res.extend(message_count.to_varint_bytes());

        // messages
        for msg in &self.messages {
            res.extend(msg.to_bytes_compact()?);
        }

        Ok(res)
    }
}

impl DeserializeCompact for AsyncPoolBootstrap {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), massa_models::ModelsError> {
        let mut cursor = 0usize;

        // message count
        let (message_count, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        // TODO cap message count https://github.com/massalabs/massa/issues/1200
        cursor += delta;

        // messages
        let mut messages = Vec::with_capacity(message_count as usize);
        for _ in 0..message_count {
            let (entry, delta) = AsyncMessage::from_bytes_compact(&buffer[cursor..])?;
            cursor += delta;
            messages.push(entry);
        }

        Ok((AsyncPoolBootstrap { messages }, cursor))
    }
}
