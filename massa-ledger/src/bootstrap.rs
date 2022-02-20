// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::collections::BTreeMap;

use massa_models::{
    array_from_slice, constants::ADDRESS_SIZE_BYTES, Address, DeserializeCompact,
    DeserializeVarInt, ModelsError, SerializeCompact, SerializeVarInt, Slot,
};
use serde::{Deserialize, Serialize};

use crate::LedgerEntry;

/// temporary ledger bootstrap structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalLedgerBootstrapState {
    /// ledger slot
    pub(crate) slot: Slot,
    /// sorted ledger
    pub(crate) sorted_ledger: BTreeMap<Address, LedgerEntry>,
}

impl SerializeCompact for FinalLedgerBootstrapState {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, massa_models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // final slot
        res.extend(self.slot.to_bytes_compact()?);

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

impl DeserializeCompact for FinalLedgerBootstrapState {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), massa_models::ModelsError> {
        let mut cursor = 0usize;

        // final slot
        let (slot, delta) = Slot::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        // ledger size
        let (ledger_size, delta) = u64::from_varint_bytes(&buffer[cursor..])?
            .try_into()
            .map_err(|_| {
                ModelsError::SerializeError("could not convert ledger size to usize".into())
            })?;
        // TODO cap the ledger size
        cursor += delta;

        // final ledger
        let mut sorted_ledger: BTreeMap<Address, LedgerEntry> = BTreeMap::new();
        cursor += delta;
        for _ in 0..ledger_size {
            // address
            let addr = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += ADDRESS_SIZE_BYTES;

            // entry
            let (entry, delta) = LedgerEntry::from_bytes_compact(&buffer[cursor..])?;
            cursor += delta;

            sorted_ledger.insert(addr, entry);
        }

        Ok((
            FinalLedgerBootstrapState {
                slot,
                sorted_ledger,
            },
            cursor,
        ))
    }
}
