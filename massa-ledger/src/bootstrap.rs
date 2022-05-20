// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Provides serializable structures for bootstrapping the `FinalLedger`

use massa_models::{
    constants::ADDRESS_SIZE_BYTES, DeserializeCompact, DeserializeVarInt, ModelsError,
    SerializeCompact, SerializeVarInt,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Represents a snapshot of the ledger state,
/// which is enough to fully bootstrap a `FinalLedger`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalLedgerBootstrapState {
    /// sorted ledger
    pub(crate) sorted_ledger: BTreeMap<Vec<u8>, Vec<u8>>,
}

/// Allows serializing the `FinalLedgerBootstrapState` to a compact binary representation
impl SerializeCompact for FinalLedgerBootstrapState {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, massa_models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // final ledger size
        let ledger_size: u64 = self.sorted_ledger.len().try_into().map_err(|_| {
            ModelsError::SerializeError("could not represent ledger size as u64".into())
        })?;
        res.extend(ledger_size.to_varint_bytes());

        // ledger elements
        for (key, entry) in &self.sorted_ledger {
            // address
            res.extend(key);

            // entry size
            let entry_size: u64 = self.sorted_ledger.len().try_into().map_err(|_| {
                ModelsError::SerializeError("could not represent entry size as u64".into())
            })?;
            res.extend(entry_size.to_varint_bytes());

            // entry
            res.extend(entry);
        }

        Ok(res)
    }
}

/// Allows deserializing a `FinalLedgerBootstrapState` from its compact binary representation
impl DeserializeCompact for FinalLedgerBootstrapState {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), massa_models::ModelsError> {
        let mut cursor = 0usize;

        // ledger size
        // TODO cap the ledger size https://github.com/massalabs/massa/issues/1200
        let (ledger_size, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        // final ledger
        let mut sorted_ledger: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
        for _ in 0..ledger_size {
            // key
            let key = &buffer[cursor..];
            cursor += ADDRESS_SIZE_BYTES;

            // entry size
            let (entry_size, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
            cursor += delta;

            // entry
            let entry = &buffer[cursor..];
            cursor += entry_size as usize;

            sorted_ledger.insert(key.to_vec(), entry.to_vec());
        }

        Ok((FinalLedgerBootstrapState { sorted_ledger }, cursor))
    }
}
