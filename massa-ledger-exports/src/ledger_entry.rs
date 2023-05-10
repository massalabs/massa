// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the structure representing an entry in the `FinalLedger`

use crate::ledger_changes::LedgerEntryUpdate;
use crate::types::{Applicable, SetOrDelete};
use massa_models::amount::{Amount, AmountDeserializer, AmountSerializer};
use massa_models::bytecode::{Bytecode, BytecodeDeserializer, BytecodeSerializer};
use massa_models::datastore::{Datastore, DatastoreDeserializer, DatastoreSerializer};
use massa_serialization::{Deserializer, SerializeError, Serializer};
use nom::error::{context, ContextError, ParseError};
use nom::sequence::tuple;
use nom::{IResult, Parser};
use serde::{Deserialize, Serialize};
use std::ops::Bound::Included;

/// Structure defining an entry associated to an address in the `FinalLedger`
#[derive(Default, Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct LedgerEntry {
    /// The balance of that entry.
    pub balance: Amount,

    /// Executable bytecode
    pub bytecode: Bytecode,

    /// A key-value store associating a hash to arbitrary bytes
    pub datastore: Datastore,
}

/// Serializer for `LedgerEntry`
pub(crate) struct LedgerEntrySerializer {
    amount_serializer: AmountSerializer,
    bytecode_serializer: BytecodeSerializer,
    datastore_serializer: DatastoreSerializer,
}

impl LedgerEntrySerializer {
    /// Creates a new `LedgerEntrySerializer`
    pub(crate) fn new() -> Self {
        Self {
            amount_serializer: AmountSerializer::new(),
            bytecode_serializer: BytecodeSerializer::new(),
            datastore_serializer: DatastoreSerializer::new(),
        }
    }
}

impl Default for LedgerEntrySerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<LedgerEntry> for LedgerEntrySerializer {
    /// ## Example
    /// ```rust,ignore
    /// // TODO: reinstate this doc-test. was ignored when these were made private
    /// use massa_serialization::Serializer;
    /// use std::collections::BTreeMap;
    /// use std::str::FromStr;
    /// use massa_models::{amount::Amount, bytecode::Bytecode};
    /// use massa_ledger_exports::{LedgerEntry, LedgerEntrySerializer};
    ///
    /// let key = "hello world".as_bytes().to_vec();
    /// let mut datastore = BTreeMap::new();
    /// datastore.insert(key, vec![1, 2, 3]);
    /// let balance = Amount::from_str("1").unwrap();
    /// let bytecode = Bytecode(vec![1, 2, 3]);
    /// let ledger_entry = LedgerEntry {
    ///    balance,
    ///    bytecode,
    ///    datastore,
    /// };
    /// let mut serialized = Vec::new();
    /// let serializer = LedgerEntrySerializer::new();
    /// serializer.serialize(&ledger_entry, &mut serialized).unwrap();
    /// ```
    fn serialize(&self, value: &LedgerEntry, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.amount_serializer.serialize(&value.balance, buffer)?;
        self.bytecode_serializer
            .serialize(&value.bytecode, buffer)?;
        self.datastore_serializer
            .serialize(&value.datastore, buffer)?;
        Ok(())
    }
}

/// Deserializer for `LedgerEntry`
pub(crate) struct LedgerEntryDeserializer {
    amount_deserializer: AmountDeserializer,
    bytecode_deserializer: BytecodeDeserializer,
    datastore_deserializer: DatastoreDeserializer,
}

impl LedgerEntryDeserializer {
    /// Creates a new `LedgerEntryDeserializer`
    pub(crate) fn new(
        max_datastore_entry_count: u64,
        max_datastore_key_length: u8,
        max_datastore_value_length: u64,
    ) -> Self {
        Self {
            amount_deserializer: AmountDeserializer::new(
                Included(Amount::MIN),
                Included(Amount::MAX),
            ),
            bytecode_deserializer: BytecodeDeserializer::new(max_datastore_value_length),
            datastore_deserializer: DatastoreDeserializer::new(
                max_datastore_entry_count,
                max_datastore_key_length,
                max_datastore_value_length,
            ),
        }
    }
}

impl Deserializer<LedgerEntry> for LedgerEntryDeserializer {
    /// ## Example
    /// ```rust,ignore
    /// // TODO: reinstate this doc-test. was ignored when these were made private
    /// use massa_serialization::{Deserializer, Serializer, DeserializeError};
    /// use std::collections::BTreeMap;
    /// use std::str::FromStr;
    /// use massa_models::{amount::Amount, bytecode::Bytecode};
    /// use massa_ledger_exports::{LedgerEntry, LedgerEntrySerializer, LedgerEntryDeserializer};
    ///
    /// let key = "hello world".as_bytes().to_vec();
    /// let mut datastore = BTreeMap::new();
    /// datastore.insert(key, vec![1, 2, 3]);
    /// let balance = Amount::from_str("1").unwrap();
    /// let bytecode = Bytecode(vec![1, 2, 3]);
    /// let ledger_entry = LedgerEntry {
    ///    balance,
    ///    bytecode,
    ///    datastore,
    /// };
    /// let mut serialized = Vec::new();
    /// let serializer = LedgerEntrySerializer::new();
    /// let deserializer = LedgerEntryDeserializer::new(10000, 255, 10000);
    /// serializer.serialize(&ledger_entry, &mut serialized).unwrap();
    /// let (rest, ledger_entry_deser) = deserializer.deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(ledger_entry, ledger_entry_deser);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], LedgerEntry, E> {
        context(
            "Failed LedgerEntry deserialization",
            tuple((
                context("Failed balance deserialization", |input| {
                    self.amount_deserializer.deserialize(input)
                }),
                context("Failed bytecode deserialization", |input| {
                    self.bytecode_deserializer.deserialize(input)
                }),
                context("Failed datastore deserialization", |input| {
                    self.datastore_deserializer.deserialize(input)
                }),
            )),
        )
        .map(|(balance, bytecode, datastore)| LedgerEntry {
            balance,
            bytecode,
            datastore,
        })
        .parse(buffer)
    }
}

/// A `LedgerEntryUpdate` can be applied to a `LedgerEntry`
impl Applicable<LedgerEntryUpdate> for LedgerEntry {
    fn apply(&mut self, update: LedgerEntryUpdate) {
        // apply updates to the balance
        update.balance.apply_to(&mut self.balance);

        // apply updates to the executable bytecode
        update.bytecode.apply_to(&mut self.bytecode);

        // iterate over all datastore updates
        for (key, value_update) in update.datastore {
            match value_update {
                // this update sets a new value to a datastore entry
                SetOrDelete::Set(v) => {
                    // insert the new value in the datastore,
                    // replacing any existing value
                    self.datastore.insert(key, v);
                }

                // this update deletes a datastore entry
                SetOrDelete::Delete => {
                    // remove that entry from the datastore if it exists
                    self.datastore.remove(&key);
                }
            }
        }
    }
}
