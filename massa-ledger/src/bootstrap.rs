// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Provides serializable structures for bootstrapping the `FinalLedger`

use crate::{
    cursor::{LedgerCursor, LedgerCursorStep},
    LedgerEntry,
};
use massa_models::{
    amount::AmountSerializer,
    array_from_slice,
    constants::{default::MAXIMUM_BYTES_MESSAGE_BOOTSTRAP, ADDRESS_SIZE_BYTES},
    Address, DeserializeCompact, DeserializeVarInt, ModelsError, SerializeCompact, SerializeVarInt,
    Serializer,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ops::Bound::{Excluded, Unbounded};

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

impl FinalLedgerBootstrapState {
    /// Get a part of the ledger
    /// Used for bootstrap
    /// Parameters:
    /// * cursor: Where we stopped in the ledger
    ///
    /// Returns:
    /// A subset of the ledger starting at `cursor` and of size `MAXIMUM_BYTES_MESSAGE_BOOTSTRAP` bytes.
    pub fn get_ledger_part(
        &self,
        cursor: Option<LedgerCursor>,
    ) -> Result<(Vec<u8>, LedgerCursor), ModelsError> {
        let mut next_cursor = cursor.unwrap_or((
            *self
                .sorted_ledger
                .first_key_value()
                .ok_or_else(|| ModelsError::BufferError("Ledger empty".into()))?
                .0,
            LedgerCursorStep::Start,
        ));
        let mut data = Vec::new();
        let amount_serializer = AmountSerializer::new();
        for (addr, entry) in self.sorted_ledger.range(next_cursor.0..) {
            // No match because we want to be able to pass in all if in one loop
            if let LedgerCursorStep::Finish = next_cursor.1 {
                next_cursor.1 = LedgerCursorStep::Start;
                next_cursor.0 = *addr;
            }
            if let LedgerCursorStep::Start = next_cursor.1 {
                data.extend(addr.to_bytes());
                next_cursor.1 = LedgerCursorStep::Balance;
                if data.len() as u32 > MAXIMUM_BYTES_MESSAGE_BOOTSTRAP {
                    return Ok((data, next_cursor));
                }
            }
            if let LedgerCursorStep::Balance = next_cursor.1 {
                data.extend(amount_serializer.serialize(&entry.parallel_balance)?);
                next_cursor.1 = LedgerCursorStep::StartDatastore;
                if data.len() as u32 > MAXIMUM_BYTES_MESSAGE_BOOTSTRAP {
                    return Ok((data, next_cursor));
                }
            }
            if let LedgerCursorStep::StartDatastore = next_cursor.1 {
                for (key, value) in &entry.datastore {
                    data.extend(key.to_bytes());
                    data.extend((value.len() as u64).to_varint_bytes());
                    data.extend(value);
                    next_cursor.1 = LedgerCursorStep::Datastore(*key);
                    if data.len() as u32 > MAXIMUM_BYTES_MESSAGE_BOOTSTRAP {
                        return Ok((data, next_cursor));
                    }
                }
            }
            if let LedgerCursorStep::Datastore(key) = next_cursor.1 {
                for (key, value) in entry.datastore.range((Excluded(key), Unbounded)) {
                    data.extend(key.to_bytes());
                    data.extend((value.len() as u64).to_varint_bytes());
                    data.extend(value);
                    next_cursor.1 = LedgerCursorStep::Datastore(*key);
                    if data.len() as u32 > MAXIMUM_BYTES_MESSAGE_BOOTSTRAP {
                        return Ok((data, next_cursor));
                    }
                }
            }
        }
        // Need to dereference because Prehashed trait is not implemented for &Address
        // TODO: Reimplement it
        Err(ModelsError::SerializeError("wip".into()))
    }
}
