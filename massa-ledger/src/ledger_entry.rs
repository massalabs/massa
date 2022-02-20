// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::ledger_changes::LedgerEntryUpdate;
use crate::types::{Applicable, SetOrDelete};
use massa_hash::hash::Hash;
use massa_hash::HASH_SIZE_BYTES;
use massa_models::{array_from_slice, Amount, DeserializeVarInt, ModelsError, SerializeVarInt};
use massa_models::{DeserializeCompact, SerializeCompact};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// structure defining a ledger entry
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub parallel_balance: Amount,
    pub bytecode: Vec<u8>,
    pub datastore: BTreeMap<Hash, Vec<u8>>,
}

/// LedgerEntryUpdate can be applied to a LedgerEntry
impl Applicable<LedgerEntryUpdate> for LedgerEntry {
    /// applies a LedgerEntryUpdate
    fn apply(&mut self, update: LedgerEntryUpdate) {
        update.parallel_balance.apply_to(&mut self.parallel_balance);
        update.bytecode.apply_to(&mut self.bytecode);
        for (key, value_update) in update.datastore {
            match value_update {
                SetOrDelete::Set(v) => {
                    self.datastore.insert(key, v);
                }
                SetOrDelete::Delete => {
                    self.datastore.remove(&key);
                }
            }
        }
    }
}

/// serialize as compact binary
impl SerializeCompact for LedgerEntry {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, massa_models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // parallel balance
        res.extend(self.parallel_balance.to_bytes_compact()?);

        // bytecode length
        let bytecode_len: u64 = self.bytecode.len().try_into().map_err(|_| {
            ModelsError::SerializeError("could not convert bytecode size to u64".into())
        })?;
        res.extend(bytecode_len.to_varint_bytes());

        // bytecode
        res.extend(&self.bytecode);

        // datastore length
        let datastore_len: u64 = self.datastore.len().try_into().map_err(|_| {
            ModelsError::SerializeError("could not convert datastore size to u64".into())
        })?;
        res.extend(datastore_len.to_varint_bytes());

        // datastore
        for (key, value) in &self.datastore {
            // key
            res.extend(key.to_bytes());

            // value length
            let value_len: u64 = value.len().try_into().map_err(|_| {
                ModelsError::SerializeError("could not convert datastore value size to u64".into())
            })?;
            res.extend(value_len.to_varint_bytes());

            // value
            res.extend(value);
        }

        Ok(res)
    }
}

impl DeserializeCompact for LedgerEntry {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), massa_models::ModelsError> {
        let mut cursor = 0usize;

        // parallel balance
        let (parallel_balance, delta) = Amount::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        // bytecode length
        let (bytecode_len, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        let bytecode_len: usize = bytecode_len.try_into().map_err(|_| {
            ModelsError::SerializeError("could not convert bytecode size to usize".into())
        })?;
        //TODO cap bytecode length
        cursor += delta;

        // bytecode
        let bytecode = if let Some(slice) = buffer.get(cursor..(cursor + (bytecode_len as usize))) {
            cursor += bytecode_len as usize;
            slice.to_vec()
        } else {
            return Err(ModelsError::DeserializeError(
                "could not deserialize ledger entry bytecode: buffer too small".into(),
            ));
        };

        // datastore length
        let (datastore_len, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        let datastore_len: usize = datastore_len.try_into().map_err(|_| {
            ModelsError::SerializeError("could not convert datastore size to usize".into())
        })?;
        //TODO cap datastore length
        cursor += delta;

        // datastore entries
        let mut datastore: BTreeMap<Hash, Vec<u8>> = BTreeMap::new();
        for _ in 0..datastore_len {
            // key
            let key = Hash::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += HASH_SIZE_BYTES;

            // value length
            let (value_len, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
            let value_len: usize = value_len.try_into().map_err(|_| {
                ModelsError::SerializeError(
                    "could not convert datastore entry value size to usize".into(),
                )
            })?;
            //TODO cap value length
            cursor += delta;

            // value
            let value = if let Some(slice) = buffer.get(cursor..(cursor + (value_len as usize))) {
                cursor += value_len as usize;
                slice.to_vec()
            } else {
                return Err(ModelsError::DeserializeError(
                    "could not deserialize ledger entry datastore value: buffer too small".into(),
                ));
            };

            datastore.insert(key, value);
        }

        Ok((
            LedgerEntry {
                parallel_balance,
                bytecode,
                datastore,
            },
            cursor,
        ))
    }
}
