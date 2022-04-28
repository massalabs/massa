// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Provides serializable structures for bootstrapping the `FinalLedger`

use crate::LedgerEntry;
use massa_models::{
    array_from_slice, constants::ADDRESS_SIZE_BYTES, Address, DeserializeCompact,
    DeserializeVarInt, ModelsError, SerializeCompact, SerializeVarInt,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Represents a snapshot of the ledger state,
/// which is enough to fully bootstrap a `FinalLedger`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalLedgerBootstrapState {
    /// sorted ledger
    pub(crate) sorted_ledger: BTreeMap<Address, LedgerEntry>,
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
        for (addr, entry) in &self.sorted_ledger {
            // address
            res.extend(addr.to_bytes());

            // entry
            res.extend(entry.to_bytes_compact()?);
        }

        Ok(res)
    }
}

/// Allows deserializing a `FinalLedgerBootstrapState` from its compact binary representation
impl DeserializeCompact for FinalLedgerBootstrapState {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), massa_models::ModelsError> {
        let mut cursor = 0usize;

        // ledger size
        let (ledger_size, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        // TODO cap the ledger size https://github.com/massalabs/massa/issues/1200
        cursor += delta;

        // final ledger
        let mut sorted_ledger: BTreeMap<Address, LedgerEntry> = BTreeMap::new();
        for _ in 0..ledger_size {
            // address
            let addr = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += ADDRESS_SIZE_BYTES;

            // entry
            let (entry, delta) = LedgerEntry::from_bytes_compact(&buffer[cursor..])?;
            cursor += delta;

            sorted_ledger.insert(addr, entry);
        }

        Ok((FinalLedgerBootstrapState { sorted_ledger }, cursor))
    }
}
