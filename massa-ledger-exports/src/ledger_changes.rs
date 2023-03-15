// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file provides structures representing changes to ledger entries

use crate::ledger_entry::{LedgerEntry, LedgerEntryDeserializer, LedgerEntrySerializer};
use crate::types::{
    Applicable, SetOrDelete, SetOrDeleteDeserializer, SetOrDeleteSerializer, SetOrKeep,
    SetOrKeepDeserializer, SetOrKeepSerializer, SetUpdateOrDelete, SetUpdateOrDeleteDeserializer,
    SetUpdateOrDeleteSerializer,
};
use massa_models::address::{Address, AddressDeserializer, AddressSerializer};
use massa_models::amount::{Amount, AmountDeserializer, AmountSerializer};
use massa_models::bytecode::{Bytecode, BytecodeDeserializer, BytecodeSerializer};
use massa_models::prehash::PreHashMap;
use massa_models::serialization::{VecU8Deserializer, VecU8Serializer};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::error::{context, ContextError, ParseError};
use nom::multi::length_count;
use nom::sequence::tuple;
use nom::{IResult, Parser};
use serde::{Deserialize, Serialize};
use std::collections::{hash_map, BTreeMap};
use std::ops::Bound::Included;

/// represents an update to one or more fields of a `LedgerEntry`
#[derive(Default, Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct LedgerEntryUpdate {
    /// change the balance
    pub balance: SetOrKeep<Amount>,
    /// change the executable bytecode
    pub bytecode: SetOrKeep<Bytecode>,
    /// change datastore entries
    pub datastore: BTreeMap<Vec<u8>, SetOrDelete<Vec<u8>>>,
}

/// Serializer for `datastore` field of `LedgerEntryUpdate`
pub struct DatastoreUpdateSerializer {
    u64_serializer: U64VarIntSerializer,
    vec_u8_serializer: VecU8Serializer,
    value_serializer: SetOrDeleteSerializer<Vec<u8>, VecU8Serializer>,
}

impl DatastoreUpdateSerializer {
    /// Creates a new `DatastoreUpdateSerializer`
    pub fn new() -> Self {
        Self {
            u64_serializer: U64VarIntSerializer::new(),
            vec_u8_serializer: VecU8Serializer::new(),
            value_serializer: SetOrDeleteSerializer::new(VecU8Serializer::new()),
        }
    }
}

impl Default for DatastoreUpdateSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<BTreeMap<Vec<u8>, SetOrDelete<Vec<u8>>>> for DatastoreUpdateSerializer {
    /// ## Example
    /// ```rust
    /// use std::collections::BTreeMap;
    /// use massa_ledger_exports::{DatastoreUpdateSerializer, SetOrDelete};
    /// use massa_serialization::Serializer;
    ///
    /// let serializer = DatastoreUpdateSerializer::new();
    /// let mut buffer = Vec::new();
    /// let mut datastore = BTreeMap::new();
    /// datastore.insert(vec![1, 2, 3], SetOrDelete::Set(vec![4, 5, 6]));
    /// datastore.insert(vec![3, 4, 5], SetOrDelete::Delete);
    /// serializer.serialize(&datastore, &mut buffer).unwrap();
    /// ```
    fn serialize(
        &self,
        value: &BTreeMap<Vec<u8>, SetOrDelete<Vec<u8>>>,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        let entry_count: u64 = value.len().try_into().map_err(|err| {
            SerializeError::GeneralError(format!(
                "too many entries in ConsensusLedgerSubset: {}",
                err
            ))
        })?;
        self.u64_serializer.serialize(&entry_count, buffer)?;
        for (key, value) in value.iter() {
            self.vec_u8_serializer.serialize(key, buffer)?;
            self.value_serializer.serialize(value, buffer)?;
        }
        Ok(())
    }
}

/// Serializer for `datastore` field of `LedgerEntryUpdate`
pub struct DatastoreUpdateDeserializer {
    length_deserializer: U64VarIntDeserializer,
    key_deserializer: VecU8Deserializer,
    value_deserializer: SetOrDeleteDeserializer<Vec<u8>, VecU8Deserializer>,
}

impl DatastoreUpdateDeserializer {
    /// Creates a new `DatastoreUpdateDeserializer`
    pub fn new(
        max_datastore_key_length: u8,
        max_datastore_value_length: u64,
        max_datastore_entry_count: u64,
    ) -> Self {
        Self {
            length_deserializer: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(max_datastore_entry_count),
            ),
            key_deserializer: VecU8Deserializer::new(
                Included(u64::MIN),
                Included(max_datastore_key_length as u64),
            ),
            value_deserializer: SetOrDeleteDeserializer::new(VecU8Deserializer::new(
                Included(u64::MIN),
                Included(max_datastore_value_length),
            )),
        }
    }
}

impl Deserializer<BTreeMap<Vec<u8>, SetOrDelete<Vec<u8>>>> for DatastoreUpdateDeserializer {
    /// ## Example
    /// ```rust
    /// use std::collections::BTreeMap;
    /// use massa_ledger_exports::{DatastoreUpdateDeserializer, DatastoreUpdateSerializer, SetOrDelete};
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    ///
    /// let serializer = DatastoreUpdateSerializer::new();
    /// let deserializer = DatastoreUpdateDeserializer::new(255, 255, 255);
    /// let mut buffer = Vec::new();
    /// let mut datastore = BTreeMap::new();
    /// datastore.insert(vec![1, 2, 3], SetOrDelete::Set(vec![4, 5, 6]));
    /// datastore.insert(vec![3, 4, 5], SetOrDelete::Delete);
    /// serializer.serialize(&datastore, &mut buffer).unwrap();
    /// let (rest, deserialized) = deserializer.deserialize::<DeserializeError>(&buffer).unwrap();
    /// assert_eq!(rest.len(), 0);
    /// assert_eq!(deserialized, datastore);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BTreeMap<Vec<u8>, SetOrDelete<Vec<u8>>>, E> {
        context(
            "Failed Datastore deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.length_deserializer.deserialize(input)
                }),
                |input| {
                    tuple((
                        context("Failed key deserialization", |input| {
                            self.key_deserializer.deserialize(input)
                        }),
                        context("Failed value deserialization", |input| {
                            self.value_deserializer.deserialize(input)
                        }),
                    ))(input)
                },
            ),
        )
        .map(|elems| elems.into_iter().collect())
        .parse(buffer)
    }
}

/// Serializer for `LedgerEntryUpdate`
pub struct LedgerEntryUpdateSerializer {
    balance_serializer: SetOrKeepSerializer<Amount, AmountSerializer>,
    bytecode_serializer: SetOrKeepSerializer<Bytecode, BytecodeSerializer>,
    datastore_serializer: DatastoreUpdateSerializer,
}

impl LedgerEntryUpdateSerializer {
    /// Creates a new `LedgerEntryUpdateSerializer`
    pub fn new() -> Self {
        Self {
            balance_serializer: SetOrKeepSerializer::new(AmountSerializer::new()),
            bytecode_serializer: SetOrKeepSerializer::new(BytecodeSerializer::new()),
            datastore_serializer: DatastoreUpdateSerializer::new(),
        }
    }
}

impl Default for LedgerEntryUpdateSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<LedgerEntryUpdate> for LedgerEntryUpdateSerializer {
    /// ## Example
    /// ```
    /// use massa_serialization::Serializer;
    /// use massa_models::{prehash::PreHashMap, address::Address, amount::Amount, bytecode::Bytecode};
    /// use std::str::FromStr;
    /// use std::collections::BTreeMap;
    /// use massa_ledger_exports::{SetOrDelete, SetOrKeep, LedgerEntryUpdate, LedgerEntryUpdateSerializer};
    ///
    /// let key = "hello world".as_bytes().to_vec();
    /// let mut datastore = BTreeMap::default();
    /// datastore.insert(key, SetOrDelete::Set(vec![1, 2, 3]));
    /// let amount = Amount::from_str("1").unwrap();
    /// let bytecode = Bytecode(vec![1, 2, 3]);
    /// let ledger_entry = LedgerEntryUpdate {
    ///    balance: SetOrKeep::Keep,
    ///    bytecode: SetOrKeep::Set(bytecode.clone()),
    ///    datastore,
    /// };
    /// let mut serialized = Vec::new();
    /// let serializer = LedgerEntryUpdateSerializer::new();
    /// serializer.serialize(&ledger_entry, &mut serialized).unwrap();
    /// ```
    fn serialize(
        &self,
        value: &LedgerEntryUpdate,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        self.balance_serializer.serialize(&value.balance, buffer)?;
        self.bytecode_serializer
            .serialize(&value.bytecode, buffer)?;
        self.datastore_serializer
            .serialize(&value.datastore, buffer)?;
        Ok(())
    }
}

/// Deserializer for `LedgerEntryUpdate`
pub struct LedgerEntryUpdateDeserializer {
    amount_deserializer: SetOrKeepDeserializer<Amount, AmountDeserializer>,
    bytecode_deserializer: SetOrKeepDeserializer<Bytecode, BytecodeDeserializer>,
    datastore_deserializer: DatastoreUpdateDeserializer,
}

impl LedgerEntryUpdateDeserializer {
    /// Creates a new `LedgerEntryUpdateDeserializer`
    pub fn new(
        max_datastore_key_length: u8,
        max_datastore_value_length: u64,
        max_datastore_entry_count: u64,
    ) -> Self {
        Self {
            amount_deserializer: SetOrKeepDeserializer::new(AmountDeserializer::new(
                Included(Amount::MIN),
                Included(Amount::MAX),
            )),
            bytecode_deserializer: SetOrKeepDeserializer::new(BytecodeDeserializer::new(
                max_datastore_value_length,
            )),
            datastore_deserializer: DatastoreUpdateDeserializer::new(
                max_datastore_key_length,
                max_datastore_value_length,
                max_datastore_entry_count,
            ),
        }
    }
}

impl Deserializer<LedgerEntryUpdate> for LedgerEntryUpdateDeserializer {
    /// ## Example
    /// ```
    /// use massa_serialization::{Deserializer, Serializer, DeserializeError};
    /// use massa_models::{prehash::PreHashMap, address::Address, amount::Amount, bytecode::Bytecode};
    /// use std::str::FromStr;
    /// use std::collections::BTreeMap;
    /// use massa_ledger_exports::{SetOrDelete, SetOrKeep, LedgerEntryUpdate, LedgerEntryUpdateSerializer, LedgerEntryUpdateDeserializer};
    ///
    /// let key = "hello world".as_bytes().to_vec();
    /// let mut datastore = BTreeMap::default();
    /// datastore.insert(key, SetOrDelete::Set(vec![1, 2, 3]));
    /// let amount = Amount::from_str("1").unwrap();
    /// let bytecode = Bytecode(vec![1, 2, 3]);
    /// let ledger_entry = LedgerEntryUpdate {
    ///    balance: SetOrKeep::Keep,
    ///    bytecode: SetOrKeep::Set(bytecode.clone()),
    ///    datastore,
    /// };
    /// let mut serialized = Vec::new();
    /// let serializer = LedgerEntryUpdateSerializer::new();
    /// let deserializer = LedgerEntryUpdateDeserializer::new(255, 10000, 10000);
    /// serializer.serialize(&ledger_entry, &mut serialized).unwrap();
    /// let (rest, ledger_entry_deser) = deserializer.deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(ledger_entry, ledger_entry_deser);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], LedgerEntryUpdate, E> {
        context(
            "Failed LedgerEntryUpdate deserialization",
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
        .map(|(balance, bytecode, datastore)| LedgerEntryUpdate {
            balance,
            bytecode,
            datastore,
        })
        .parse(buffer)
    }
}

impl Applicable<LedgerEntryUpdate> for LedgerEntryUpdate {
    /// extends the `LedgerEntryUpdate` with another one
    fn apply(&mut self, update: LedgerEntryUpdate) {
        self.balance.apply(update.balance);
        self.bytecode.apply(update.bytecode);
        self.datastore.extend(update.datastore);
    }
}

/// represents a list of changes to multiple ledger entries
#[derive(Default, Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct LedgerChanges(
    pub PreHashMap<Address, SetUpdateOrDelete<LedgerEntry, LedgerEntryUpdate>>,
);

/// `LedgerChanges` serializer
pub struct LedgerChangesSerializer {
    u64_serializer: U64VarIntSerializer,
    address_serializer: AddressSerializer,
    entry_serializer: SetUpdateOrDeleteSerializer<
        LedgerEntry,
        LedgerEntryUpdate,
        LedgerEntrySerializer,
        LedgerEntryUpdateSerializer,
    >,
}

impl LedgerChangesSerializer {
    /// Creates a new `LedgerChangesSerializer`
    pub fn new() -> Self {
        Self {
            u64_serializer: U64VarIntSerializer::new(),
            address_serializer: AddressSerializer::new(),
            entry_serializer: SetUpdateOrDeleteSerializer::new(
                LedgerEntrySerializer::new(),
                LedgerEntryUpdateSerializer::new(),
            ),
        }
    }
}

impl Default for LedgerChangesSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<LedgerChanges> for LedgerChangesSerializer {
    /// ## Example
    /// ```
    /// use massa_serialization::Serializer;
    /// use massa_ledger_exports::{LedgerEntry, SetUpdateOrDelete, LedgerChanges, LedgerChangesSerializer};
    /// use std::str::FromStr;
    /// use std::collections::BTreeMap;
    /// use massa_models::{amount::Amount, address::Address, bytecode::Bytecode};
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
    /// let mut changes = LedgerChanges::default();
    /// changes.0.insert(
    ///    Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
    ///    SetUpdateOrDelete::Set(ledger_entry),
    /// );
    /// LedgerChangesSerializer::new().serialize(&changes, &mut serialized).unwrap();
    /// ```
    fn serialize(&self, value: &LedgerChanges, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        let entry_count: u64 = value.0.len().try_into().map_err(|err| {
            SerializeError::GeneralError(format!("too many entries in LedgerChanges: {}", err))
        })?;
        self.u64_serializer.serialize(&entry_count, buffer)?;
        for (address, data) in value.0.iter() {
            self.address_serializer.serialize(address, buffer)?;
            self.entry_serializer.serialize(data, buffer)?;
        }
        Ok(())
    }
}

/// `LedgerChanges` deserializer
pub struct LedgerChangesDeserializer {
    length_deserializer: U64VarIntDeserializer,
    address_deserializer: AddressDeserializer,
    entry_deserializer: SetUpdateOrDeleteDeserializer<
        LedgerEntry,
        LedgerEntryUpdate,
        LedgerEntryDeserializer,
        LedgerEntryUpdateDeserializer,
    >,
}

impl LedgerChangesDeserializer {
    /// Creates a new `LedgerChangesDeserializer`
    pub fn new(
        max_ledger_changes_count: u64,
        max_datastore_key_length: u8,
        max_datastore_value_length: u64,
        max_datastore_entry_count: u64,
    ) -> Self {
        Self {
            length_deserializer: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(max_ledger_changes_count),
            ),
            address_deserializer: AddressDeserializer::new(),
            entry_deserializer: SetUpdateOrDeleteDeserializer::new(
                LedgerEntryDeserializer::new(
                    max_datastore_entry_count,
                    max_datastore_key_length,
                    max_datastore_value_length,
                ),
                LedgerEntryUpdateDeserializer::new(
                    max_datastore_key_length,
                    max_datastore_value_length,
                    max_datastore_entry_count,
                ),
            ),
        }
    }
}

impl Deserializer<LedgerChanges> for LedgerChangesDeserializer {
    /// ## Example
    /// ```
    /// use massa_serialization::{Deserializer, Serializer, DeserializeError};
    /// use massa_ledger_exports::{LedgerEntry, SetUpdateOrDelete, LedgerChanges, LedgerChangesSerializer, LedgerChangesDeserializer};
    /// use std::str::FromStr;
    /// use std::collections::BTreeMap;
    /// use massa_models::{amount::Amount, address::Address, bytecode::Bytecode};
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
    /// let mut changes = LedgerChanges::default();
    /// changes.0.insert(
    ///    Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
    ///    SetUpdateOrDelete::Set(ledger_entry),
    /// );
    /// LedgerChangesSerializer::new().serialize(&changes, &mut serialized).unwrap();
    /// let (rest, changes_deser) = LedgerChangesDeserializer::new(255, 255, 10000, 10000).deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(changes, changes_deser);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], LedgerChanges, E> {
        context(
            "Failed LedgerChanges deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.length_deserializer.deserialize(input)
                }),
                tuple((
                    context("Failed address deserialization", |input| {
                        self.address_deserializer.deserialize(input)
                    }),
                    context("Failed entry deserialization", |input| {
                        self.entry_deserializer.deserialize(input)
                    }),
                )),
            ),
        )
        .map(|res| LedgerChanges(res.into_iter().collect()))
        .parse(buffer)
    }
}

impl Applicable<LedgerChanges> for LedgerChanges {
    /// extends the current `LedgerChanges` with another one
    fn apply(&mut self, changes: LedgerChanges) {
        for (addr, change) in changes.0 {
            match self.0.entry(addr) {
                hash_map::Entry::Occupied(mut occ) => {
                    // apply incoming change if a change on this entry already exists
                    occ.get_mut().apply(change);
                }
                hash_map::Entry::Vacant(vac) => {
                    // otherwise insert the incoming change
                    vac.insert(change);
                }
            }
        }
    }
}

impl LedgerChanges {
    /// Get an item from the `LedgerChanges`
    pub fn get(
        &self,
        addr: &Address,
    ) -> Option<&SetUpdateOrDelete<LedgerEntry, LedgerEntryUpdate>> {
        self.0.get(addr)
    }

    /// Retrieves all the bytcode updates contained in the current changes
    pub fn get_byetcode_updates(&self) -> Vec<Bytecode> {
        let mut v = Vec::new();
        for (_addr, change) in self.0.iter() {
            match change {
                SetUpdateOrDelete::Set(entry) => v.push(entry.bytecode.clone()),
                SetUpdateOrDelete::Update(entry_update) => {
                    if let SetOrKeep::Set(bytecode) = entry_update.bytecode.clone() {
                        v.push(bytecode);
                    }
                }
                SetUpdateOrDelete::Delete => (),
            }
        }
        v
    }

    /// Create a new, empty address.
    /// Overwrites the address if it is already there.
    pub fn create_address(&mut self, address: &Address) {
        self.0
            .insert(*address, SetUpdateOrDelete::Set(LedgerEntry::default()));
    }

    /// Tries to return the balance of an entry
    /// or gets it from a function if the entry's status is unknown.
    ///
    /// This function is used as an optimization:
    /// if the value can be deduced unambiguously from the `LedgerChanges`,
    /// no need to dig further (for example in the `FinalLedger`).
    ///
    /// # Arguments
    /// * `addr`: address for which to get the value
    /// * `f`: fallback function with no arguments and returning `Option<Amount>`
    ///
    /// # Returns
    /// * Some(v) if a value is present, where v is a copy of the value
    /// * None if the value is absent
    /// * f() if the value is unknown
    pub fn get_balance_or_else<F: FnOnce() -> Option<Amount>>(
        &self,
        addr: &Address,
        f: F,
    ) -> Option<Amount> {
        // Get the changes for the provided address
        match self.0.get(addr) {
            // This entry is being replaced by a new one: get the balance from the new entry
            Some(SetUpdateOrDelete::Set(v)) => Some(v.balance),

            // This entry is being updated
            Some(SetUpdateOrDelete::Update(LedgerEntryUpdate { balance, .. })) => match balance {
                // The update sets a new balance: return it
                SetOrKeep::Set(v) => Some(*v),
                // The update keeps the old balance.
                // We therefore have no info on the absolute value of the balance.
                // We call the fallback function and return its output.
                SetOrKeep::Keep => f(),
            },

            // This entry is being deleted: return None.
            Some(SetUpdateOrDelete::Delete) => None,

            // This entry is not being changed.
            // We therefore have no info on the absolute value of the balance.
            // We call the fallback function and return its output.
            None => f(),
        }
    }

    /// Tries to return the executable bytecode of an entry
    /// or gets it from a function if the entry's status is unknown.
    ///
    /// This function is used as an optimization:
    /// if the value can be deduced unambiguously from the `LedgerChanges`,
    /// no need to dig further (for example in the `FinalLedger`).
    ///
    /// # Arguments
    /// * `addr`: address for which to get the value
    /// * `f`: fallback function with no arguments and returning `Option<Vec<u8>>`
    ///
    /// # Returns
    /// * Some(v) if a value is present, where v is a copy of the value
    /// * None if the value is absent
    /// * f() if the value is unknown
    pub fn get_bytecode_or_else<F: FnOnce() -> Option<Bytecode>>(
        &self,
        addr: &Address,
        f: F,
    ) -> Option<Bytecode> {
        // Get the changes to the provided address
        match self.0.get(addr) {
            // This entry is being replaced by a new one: get the bytecode from the new entry
            Some(SetUpdateOrDelete::Set(v)) => Some(v.bytecode.clone()),

            // This entry is being updated
            Some(SetUpdateOrDelete::Update(LedgerEntryUpdate { bytecode, .. })) => match bytecode {
                // The update sets a new bytecode: return it
                SetOrKeep::Set(v) => Some(v.clone()),

                // The update keeps the old bytecode.
                // We therefore have no info on the absolute value of the bytecode.
                // We call the fallback function and return its output.
                SetOrKeep::Keep => f(),
            },

            // This entry is being deleted: return None.
            Some(SetUpdateOrDelete::Delete) => None,

            // This entry is not being changed.
            // We therefore have no info on the absolute contents of the bytecode.
            // We call the fallback function and return its output.
            None => f(),
        }
    }

    /// Tries to return whether an entry exists
    /// or gets the information from a function if the entry's status is unknown.
    ///
    /// This function is used as an optimization:
    /// if the result can be deduced unambiguously from the `LedgerChanges`,
    /// no need to dig further (for example in the `FinalLedger`).
    ///
    /// # Arguments
    /// * `addr`: address to search for
    /// * `f`: fallback function with no arguments and returning a boolean
    ///
    /// # Returns
    /// * true if the entry exists
    /// * false if the value is absent
    /// * f() if the value's existence is unknown
    pub fn entry_exists_or_else<F: FnOnce() -> bool>(&self, addr: &Address, f: F) -> bool {
        // Get the changes for the provided address
        match self.0.get(addr) {
            // The entry is being replaced by a new one: it exists
            Some(SetUpdateOrDelete::Set(_)) => true,

            // The entry is being updated:
            // assume it exists because it will be created on update if it doesn't
            Some(SetUpdateOrDelete::Update(_)) => true,

            // The entry is being deleted: it doesn't exist anymore
            Some(SetUpdateOrDelete::Delete) => false,

            // This entry is not being changed.
            // We therefore have no info on its existence.
            // We call the fallback function and return its output.
            None => f(),
        }
    }

    /// Set the balance of an address.
    /// If the address doesn't exist, its ledger entry is created.
    ///
    /// # Arguments
    /// * `addr`: target address
    /// * `balance`: balance to set for the provided address
    pub fn set_balance(&mut self, addr: Address, balance: Amount) {
        // Get the changes for the entry associated to the provided address
        match self.0.entry(addr) {
            // That entry is being changed
            hash_map::Entry::Occupied(mut occ) => {
                match occ.get_mut() {
                    // The entry is being replaced by a new one
                    SetUpdateOrDelete::Set(v) => {
                        // update the balance of the replacement entry
                        v.balance = balance;
                    }

                    // The entry is being updated
                    SetUpdateOrDelete::Update(u) => {
                        // Make sure the update sets the balance of the entry to its new value
                        u.balance = SetOrKeep::Set(balance);
                    }

                    // The entry is being deleted
                    d @ SetUpdateOrDelete::Delete => {
                        // Replace that deletion with a replacement by a new default entry
                        // for which the balance was properly set
                        *d = SetUpdateOrDelete::Set(LedgerEntry {
                            balance,
                            ..Default::default()
                        });
                    }
                }
            }

            // This entry is not being changed
            hash_map::Entry::Vacant(vac) => {
                // Induce an Update to the entry that sets the balance to its new value
                vac.insert(SetUpdateOrDelete::Update(LedgerEntryUpdate {
                    balance: SetOrKeep::Set(balance),
                    ..Default::default()
                }));
            }
        }
    }

    /// Set the executable bytecode of an address.
    /// If the address doesn't exist, its ledger entry is created.
    ///
    /// # Parameters
    /// * `addr`: target address
    /// * `bytecode`: executable bytecode to assign to that address
    pub fn set_bytecode(&mut self, addr: Address, bytecode: Bytecode) {
        // Get the current changes being applied to the entry associated to that address
        match self.0.entry(addr) {
            // There are changes currently being applied to the entry
            hash_map::Entry::Occupied(mut occ) => {
                match occ.get_mut() {
                    // The entry is being replaced by a new one
                    SetUpdateOrDelete::Set(v) => {
                        // update the bytecode of the replacement entry
                        v.bytecode = bytecode;
                    }

                    // The entry is being updated
                    SetUpdateOrDelete::Update(u) => {
                        // Ensure that the update includes setting the bytecode to its new value
                        u.bytecode = SetOrKeep::Set(bytecode);
                    }

                    // The entry is being deleted
                    d @ SetUpdateOrDelete::Delete => {
                        // Replace that deletion with a replacement by a new default entry
                        // for which the bytecode was properly set
                        *d = SetUpdateOrDelete::Set(LedgerEntry {
                            bytecode,
                            ..Default::default()
                        });
                    }
                }
            }

            // This entry is not being changed
            hash_map::Entry::Vacant(vac) => {
                // Induce an Update to the entry that sets the bytecode to its new value
                vac.insert(SetUpdateOrDelete::Update(LedgerEntryUpdate {
                    bytecode: SetOrKeep::Set(bytecode),
                    ..Default::default()
                }));
            }
        }
    }

    /// Tries to return a datastore entry for a given address,
    /// or gets it from a function if the value's status is unknown.
    ///
    /// This function is used as an optimization:
    /// if the result can be deduced unambiguously from the `LedgerChanges`,
    /// no need to dig further (for example in the `FinalLedger`).
    ///
    /// # Arguments
    /// * `addr`: target address
    /// * `key`: datastore key
    /// * `f`: fallback function with no arguments and returning `Option<Vec<u8>>`
    ///
    /// # Returns
    /// * Some(v) if the value was found, where v is a copy of the value
    /// * None if the value is absent
    /// * f() if the value is unknown
    pub fn get_data_entry_or_else<F: FnOnce() -> Option<Vec<u8>>>(
        &self,
        addr: &Address,
        key: &[u8],
        f: F,
    ) -> Option<Vec<u8>> {
        // Get the current changes being applied to the ledger entry associated to that address
        match self.0.get(addr) {
            // This ledger entry is being replaced by a new one:
            // get the datastore entry from the new ledger entry
            Some(SetUpdateOrDelete::Set(v)) => v.datastore.get(key).cloned(),

            // This ledger entry is being updated
            Some(SetUpdateOrDelete::Update(LedgerEntryUpdate { datastore, .. })) => {
                // Get the update being applied to that datastore entry
                match datastore.get(key) {
                    // A new datastore value is being set: return a clone of it
                    Some(SetOrDelete::Set(v)) => Some(v.clone()),

                    // This datastore entry is being deleted: return None
                    Some(SetOrDelete::Delete) => None,

                    // There are no changes to this particular datastore entry.
                    // We therefore have no info on the absolute contents of the datastore entry.
                    // We call the fallback function and return its output.
                    None => f(),
                }
            }

            // This ledger entry is being deleted: return None
            Some(SetUpdateOrDelete::Delete) => None,

            // This ledger entry is not being changed.
            // We therefore have no info on the absolute contents of its datastore entry.
            // We call the fallback function and return its output.
            None => f(),
        }
    }

    /// Tries to return whether there is a change on a given address in the ledger changes
    /// and optionally if a datastore key modification also exists in the address's datastore.
    ///
    /// # Arguments
    /// * `addr`: target address
    /// * `key`: optional datastore key
    ///
    /// # Returns
    /// * true if the address and, optionally the datastore key, exists in the ledger changes
    pub fn has_changes(&self, addr: &Address, key: Option<Vec<u8>>) -> bool {
        // Get the current changes being applied to the ledger entry associated to that address
        match self.0.get(addr) {
            // This ledger entry is being replaced by a new one:
            // check if the new ledger entry has a datastore entry for the provided key
            Some(SetUpdateOrDelete::Set(v)) => key.map_or(true, |k| v.datastore.contains_key(&k)),

            // This ledger entry is being updated
            Some(SetUpdateOrDelete::Update(LedgerEntryUpdate { datastore, .. })) => {
                // Check if the update being applied to that datastore entry
                key.map_or(true, |k| datastore.contains_key(&k))
            }

            // This ledger entry is being deleted: return true
            Some(SetUpdateOrDelete::Delete) => true,

            // This ledger entry is not being changed.
            None => false,
        }
    }

    /// Tries to return whether a datastore entry exists for a given address,
    /// or gets it from a function if the datastore entry's status is unknown.
    ///
    /// This function is used as an optimization:
    /// if the result can be deduced unambiguously from the `LedgerChanges`,
    /// no need to dig further (for example in the `FinalLedger`).
    ///
    /// # Arguments
    /// * `addr`: target address
    /// * `key`: datastore key
    /// * `f`: fallback function with no arguments and returning a boolean
    ///
    /// # Returns
    /// * true if the ledger entry exists and the key is present in its datastore
    /// * false if the ledger entry is absent, or if the key is not in its datastore
    /// * f() if the existence of the ledger entry or datastore entry is unknown
    pub fn has_data_entry_or_else<F: FnOnce() -> bool>(
        &self,
        addr: &Address,
        key: &[u8],
        f: F,
    ) -> bool {
        // Get the current changes being applied to the ledger entry associated to that address
        match self.0.get(addr) {
            // This ledger entry is being replaced by a new one:
            // check if the replacement ledger entry has the key in its datastore
            Some(SetUpdateOrDelete::Set(v)) => v.datastore.contains_key(key),

            // This ledger entry is being updated
            Some(SetUpdateOrDelete::Update(LedgerEntryUpdate { datastore, .. })) => {
                // Get the update being applied to that datastore entry
                match datastore.get(key) {
                    // A new datastore value is being set: the datastore entry exists
                    Some(SetOrDelete::Set(_)) => true,

                    // The datastore entry is being deletes: it doesn't exist anymore
                    Some(SetOrDelete::Delete) => false,

                    // There are no changes to this particular datastore entry.
                    // We therefore have no info on its existence.
                    // We call the fallback function and return its output.
                    None => f(),
                }
            }

            // This ledger entry is being deleted: it has no datastore anymore
            Some(SetUpdateOrDelete::Delete) => false,

            // This ledger entry is not being changed.
            // We therefore have no info on its datastore.
            // We call the fallback function and return its output.
            None => f(),
        }
    }

    /// Set a datastore entry for a given address.
    /// If the address doesn't exist, its ledger entry is created.
    /// If the datastore entry exists, its value is replaced, otherwise it is created.
    ///
    /// # Arguments
    /// * `addr`: target address
    /// * `key`: datastore key
    /// * `data`: datastore value to set
    pub fn set_data_entry(&mut self, addr: Address, key: Vec<u8>, data: Vec<u8>) {
        // Get the changes being applied to the ledger entry associated to that address
        match self.0.entry(addr) {
            // There are changes currently being applied to the ledger entry
            hash_map::Entry::Occupied(mut occ) => {
                match occ.get_mut() {
                    // The ledger entry is being replaced by a new one
                    SetUpdateOrDelete::Set(v) => {
                        // Insert the value in the datastore of the replacement entry
                        // Any existing value is overwritten
                        v.datastore.insert(key, data);
                    }

                    // The ledger entry is being updated
                    SetUpdateOrDelete::Update(u) => {
                        // Ensure that the update includes setting the datastore entry
                        u.datastore.insert(key, SetOrDelete::Set(data));
                    }

                    // The ledger entry is being deleted
                    d @ SetUpdateOrDelete::Delete => {
                        // Replace that ledger entry deletion with a replacement by a new default ledger entry
                        // for which the datastore contains the (key, value) to insert.
                        *d = SetUpdateOrDelete::Set(LedgerEntry {
                            datastore: vec![(key, data)].into_iter().collect(),
                            ..Default::default()
                        });
                    }
                }
            }

            // This ledger entry is not being changed
            hash_map::Entry::Vacant(vac) => {
                // Induce an Update to the ledger entry that sets the datastore entry to the desired value
                vac.insert(SetUpdateOrDelete::Update(LedgerEntryUpdate {
                    datastore: vec![(key, SetOrDelete::Set(data))].into_iter().collect(),
                    ..Default::default()
                }));
            }
        }
    }

    /// Deletes a datastore entry for a given address.
    /// Does nothing if the entry is missing.
    ///
    /// # Arguments
    /// * `addr`: target address
    /// * `key`: datastore key
    pub fn delete_data_entry(&mut self, addr: Address, key: Vec<u8>) {
        // Get the changes being applied to the ledger entry associated to that address
        match self.0.entry(addr) {
            // There are changes currently being applied to the ledger entry
            hash_map::Entry::Occupied(mut occ) => {
                match occ.get_mut() {
                    // The ledger entry is being replaced by a new one
                    SetUpdateOrDelete::Set(v) => {
                        // Delete the entry in the datastore of the replacement entry
                        v.datastore.remove(&key);
                    }

                    // The ledger entry is being updated
                    SetUpdateOrDelete::Update(u) => {
                        // Ensure that the update includes deleting the datastore entry
                        u.datastore.insert(key, SetOrDelete::Delete);
                    }

                    // The ledger entry is being deleted
                    SetUpdateOrDelete::Delete => {
                        // Do nothing because the whole ledger entry is being deleted
                    }
                }
            }

            // This ledger entry is not being changed
            hash_map::Entry::Vacant(vac) => {
                // Induce an Update to the ledger entry that deletes the datastore entry
                vac.insert(SetUpdateOrDelete::Update(LedgerEntryUpdate {
                    datastore: vec![(key, SetOrDelete::Delete)].into_iter().collect(),
                    ..Default::default()
                }));
            }
        }
    }
}
