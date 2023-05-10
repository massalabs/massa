// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::{
    address::{Address, AddressDeserializer, AddressSerializer},
    amount::{Amount, AmountDeserializer, AmountSerializer},
    error::ModelsError,
    error::ModelsResult as Result,
    prehash::{PreHashMap, PreHashSet},
};
use core::usize;
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    branch::alt,
    bytes::complete::tag,
    combinator::value,
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};
use serde::{Deserialize, Serialize};
use std::collections::hash_map;
use std::ops::Bound::Included;

/// a consensus ledger entry
#[derive(Debug, Default, Deserialize, Clone, Copy, Serialize)]
pub struct LedgerData {
    /// the balance in coins
    pub balance: Amount,
}

/// Basic serializer for `LedgerData`
#[derive(Default)]
pub(crate) struct LedgerDataSerializer {
    amount_serializer: AmountSerializer,
}

impl LedgerDataSerializer {
    /// Creates a `LedgerDataSerializer`
    pub(crate) fn new() -> Self {
        Self {
            amount_serializer: AmountSerializer::new(),
        }
    }
}

impl Serializer<LedgerData> for LedgerDataSerializer {
    /// ## Example:
    /// ```rust,ignore
    /// // TODO: reinstate this doc-test. was ignored when these were made private
    /// use massa_models::amount::Amount;
    /// use massa_serialization::Serializer;
    /// use std::str::FromStr;
    /// use massa_models::ledger::{LedgerData, LedgerDataSerializer};
    ///
    /// let ledger_data = LedgerData {
    ///    balance: Amount::from_str("1349").unwrap(),
    /// };
    /// let mut buffer = Vec::new();
    /// LedgerDataSerializer::new().serialize(&ledger_data, &mut buffer).unwrap();
    /// ```
    fn serialize(&self, value: &LedgerData, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.amount_serializer.serialize(&value.balance, buffer)?;
        Ok(())
    }
}

/// Basic deserializer for `LedgerData`
pub(crate) struct LedgerDataDeserializer {
    amount_deserializer: AmountDeserializer,
}

impl LedgerDataDeserializer {
    /// Creates a `LedgerDataDeserializer`
    pub(crate) fn new() -> Self {
        Self {
            amount_deserializer: AmountDeserializer::new(
                Included(Amount::MIN),
                Included(Amount::MAX),
            ),
        }
    }
}

impl Default for LedgerDataDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<LedgerData> for LedgerDataDeserializer {
    /// ## Example:
    /// ```rust,ignore
    /// // TODO: reinstate this doc-test. was ignored when these were made private
    /// use massa_models::amount::Amount;
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use std::str::FromStr;
    /// use massa_models::ledger::{LedgerData, LedgerDataDeserializer, LedgerDataSerializer};
    ///
    /// let ledger_data = LedgerData {
    ///    balance: Amount::from_str("1349").unwrap(),
    /// };
    /// let mut buffer = Vec::new();
    /// LedgerDataSerializer::new().serialize(&ledger_data, &mut buffer).unwrap();
    /// let (rest, ledger_data_deserialized) = LedgerDataDeserializer::new().deserialize::<DeserializeError>(&buffer).unwrap();
    /// assert_eq!(rest.len(), 0);
    /// assert_eq!(ledger_data.balance, ledger_data_deserialized.balance);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], LedgerData, E> {
        context("Failed LedgerData deserialization", |input| {
            self.amount_deserializer.deserialize(input)
        })
        .map(|balance| LedgerData { balance })
        .parse(buffer)
    }
}

impl LedgerData {
    /// new `LedgerData` from an initial balance
    pub(crate) fn new(starting_balance: Amount) -> LedgerData {
        LedgerData {
            balance: starting_balance,
        }
    }

    /// apply a `LedgerChange` for an entry
    /// Can fail in overflow or underflow occur
    pub(crate) fn apply_change(&mut self, change: &LedgerChange) -> Result<()> {
        if change.balance_increment {
            self.balance = self
                .balance
                .checked_add(change.balance_delta)
                .ok_or_else(|| {
                    ModelsError::InvalidLedgerChange(
                        "balance overflow in LedgerData::apply_change".into(),
                    )
                })?;
        } else {
            self.balance = self
                .balance
                .checked_sub(change.balance_delta)
                .ok_or_else(|| {
                    ModelsError::InvalidLedgerChange(
                        "balance overflow in LedgerData::apply_change".into(),
                    )
                })?;
        }
        Ok(())
    }

    /// returns true if the balance is zero
    pub(crate) fn is_nil(&self) -> bool {
        self.balance == Amount::default()
    }
}

/// A balance change that can be applied to an address
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LedgerChange {
    /// Amount to add or subtract
    pub(crate) balance_delta: Amount,
    /// whether to increment or decrements balance of delta
    pub(crate) balance_increment: bool,
}

impl Default for LedgerChange {
    fn default() -> Self {
        LedgerChange {
            balance_delta: Amount::default(),
            balance_increment: true,
        }
    }
}

/// Basic serializer for `LedgerChange`
#[derive(Default)]
pub(crate) struct LedgerChangeSerializer {
    amount_serializer: AmountSerializer,
}

impl LedgerChangeSerializer {
    /// Creates a `LedgerChangeSerializer`
    pub(crate) fn new() -> Self {
        Self {
            amount_serializer: AmountSerializer::new(),
        }
    }
}

impl Serializer<LedgerChange> for LedgerChangeSerializer {
    /// ## Example
    /// ```rust,ignore
    /// // TODO: reinstate this doc-test. was ignored when these were made private
    /// use massa_models::{address::Address, amount::Amount};
    /// use std::str::FromStr;
    /// use massa_models::ledger::{LedgerChange, LedgerChangeSerializer};
    /// use massa_serialization::Serializer;
    /// let ledger_change = LedgerChange {
    ///   balance_delta: Amount::from_str("1149").unwrap(),
    ///   balance_increment: true
    /// };
    /// let mut serialized = Vec::new();
    /// LedgerChangeSerializer::new().serialize(&ledger_change, &mut serialized).unwrap();
    /// ```
    fn serialize(&self, value: &LedgerChange, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        buffer.push(u8::from(value.balance_increment));
        self.amount_serializer
            .serialize(&value.balance_delta, buffer)?;
        Ok(())
    }
}

/// Basic deserializer for `LedgerChange`
pub(crate) struct LedgerChangeDeserializer {
    amount_deserializer: AmountDeserializer,
}

impl LedgerChangeDeserializer {
    /// Creates a `LedgerChangeDeserializer`
    pub(crate) fn new() -> Self {
        Self {
            amount_deserializer: AmountDeserializer::new(
                Included(Amount::MIN),
                Included(Amount::MAX),
            ),
        }
    }
}

impl Default for LedgerChangeDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<LedgerChange> for LedgerChangeDeserializer {
    /// ## Example
    /// ```rust,ignore
    /// // TODO: reinstate this doc-test. was ignored when these were made private
    /// use massa_models::{address::Address, amount::Amount};
    /// use std::str::FromStr;
    /// use massa_models::ledger::{LedgerChange, LedgerChangeDeserializer, LedgerChangeSerializer};
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// let ledger_change = LedgerChange {
    ///   balance_delta: Amount::from_str("1149").unwrap(),
    ///   balance_increment: true
    /// };
    /// let mut serialized = Vec::new();
    /// LedgerChangeSerializer::new().serialize(&ledger_change, &mut serialized).unwrap();
    /// let (rest, serialized) = LedgerChangeDeserializer::new().deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert_eq!(rest.len(), 0);
    /// assert_eq!(ledger_change.balance_delta, serialized.balance_delta);
    /// assert_eq!(ledger_change.balance_increment, serialized.balance_increment);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], LedgerChange, E> {
        context(
            "Failed LedgerChange deserialization",
            tuple((
                context(
                    "Failed balance_increment deserialization",
                    alt((value(true, tag(&[1u8])), value(false, tag(&[0u8])))),
                ),
                context("Failed balance_delta deserialization", |input| {
                    self.amount_deserializer.deserialize(input)
                }),
            )),
        )
        .map(|(balance_increment, balance_delta)| LedgerChange {
            balance_delta,
            balance_increment,
        })
        .parse(buffer)
    }
}

impl LedgerChange {
    /// Applies another ledger change on top of self
    pub(crate) fn chain(&mut self, change: &LedgerChange) -> Result<(), ModelsError> {
        if self.balance_increment == change.balance_increment {
            self.balance_delta = self
                .balance_delta
                .checked_add(change.balance_delta)
                .ok_or_else(|| {
                    ModelsError::InvalidLedgerChange("overflow in LedgerChange::chain".into())
                })?;
        } else if change.balance_delta > self.balance_delta {
            self.balance_delta = change
                .balance_delta
                .checked_sub(self.balance_delta)
                .ok_or_else(|| {
                    ModelsError::InvalidLedgerChange("underflow in LedgerChange::chain".into())
                })?;
            self.balance_increment = !self.balance_increment;
        } else {
            self.balance_delta = self
                .balance_delta
                .checked_sub(change.balance_delta)
                .ok_or_else(|| {
                    ModelsError::InvalidLedgerChange("underflow in LedgerChange::chain".into())
                })?;
        }
        if self.balance_delta == Amount::default() {
            self.balance_increment = true;
        }
        Ok(())
    }

    /// true if the change is 0
    pub(crate) fn is_nil(&self) -> bool {
        self.balance_delta == Amount::default()
    }
}

/// Map an address to a `LedgerChange`
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct LedgerChanges(pub PreHashMap<Address, LedgerChange>);

/// Basic serializer for `LedgerChanges`
pub(crate) struct LedgerChangesSerializer {
    length_serializer: U64VarIntSerializer,
    address_serializer: AddressSerializer,
    ledger_change_serializer: LedgerChangeSerializer,
}

impl LedgerChangesSerializer {
    /// Creates a `LedgerChangesSerializer`
    pub(crate) fn new() -> Self {
        Self {
            length_serializer: U64VarIntSerializer::new(),
            address_serializer: AddressSerializer::new(),
            ledger_change_serializer: LedgerChangeSerializer::new(),
        }
    }
}

impl Default for LedgerChangesSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<LedgerChanges> for LedgerChangesSerializer {
    fn serialize(&self, value: &LedgerChanges, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.length_serializer
            .serialize(&(value.0.len() as u64), buffer)?;
        for (address, change) in value.0.iter() {
            self.address_serializer.serialize(address, buffer)?;
            self.ledger_change_serializer.serialize(change, buffer)?;
        }
        Ok(())
    }
}

/// Basic deserializer for `LedgerChanges`
pub(crate) struct LedgerChangesDeserializer {
    length_deserializer: U64VarIntDeserializer,
    address_deserializer: AddressDeserializer,
    ledger_change_deserializer: LedgerChangeDeserializer,
}

impl LedgerChangesDeserializer {
    /// Creates a `LedgerChangesDeserializer`
    pub(crate) fn new(max_ledger_changes_count: u64) -> Self {
        Self {
            length_deserializer: U64VarIntDeserializer::new(
                Included(0),
                Included(max_ledger_changes_count),
            ),
            address_deserializer: AddressDeserializer::new(),
            ledger_change_deserializer: LedgerChangeDeserializer::new(),
        }
    }
}

impl Deserializer<LedgerChanges> for LedgerChangesDeserializer {
    /// ## Example
    /// ```rust,ignore
    /// // TODO: reinstate this doc-test. was ignored when these were made private
    /// # use massa_models::{address::Address, amount::Amount};
    /// # use std::str::FromStr;
    /// use massa_models::ledger::{LedgerChange, LedgerChanges, LedgerChangesDeserializer, LedgerChangeSerializer, LedgerChangesSerializer};
    /// # use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// # let ledger_changes = LedgerChanges(vec![
    /// #   (
    /// #       Address::from_str("AU12hgh5ULW9o8fJE9muLNXhQENaUUswQbxPyDSq8ridnDGu5gRiJ").unwrap(),
    /// #       LedgerChange {
    /// #           balance_delta: Amount::from_str("1149").unwrap(),
    /// #           balance_increment: true
    /// #       },
    /// #   ),
    /// #   (
    /// #       Address::from_str("AU12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G").unwrap(),
    /// #       LedgerChange {
    /// #           balance_delta: Amount::from_str("1020").unwrap(),
    /// #           balance_increment: true
    /// #       },
    /// #   )
    /// # ].into_iter().collect());
    /// let mut serialized = Vec::new();
    /// let ledger_change_serializer = LedgerChangeSerializer::new();
    /// LedgerChangesSerializer::new().serialize(&ledger_changes, &mut serialized).unwrap();
    /// let (_, res) = LedgerChangesDeserializer::new(10000).deserialize::<DeserializeError>(&serialized).unwrap();
    /// for (address, data) in &ledger_changes.0 {
    ///    let mut data_serialized = Vec::new();
    ///    ledger_change_serializer.serialize(data, &mut data_serialized).unwrap();
    ///    assert!(res.0.iter().filter(|(addr, dta)| {
    ///      let mut dta_serialized = Vec::new();
    ///      ledger_change_serializer.serialize(dta, &mut dta_serialized).unwrap();
    ///      &address == addr && dta_serialized == data_serialized
    ///     }).count() == 1);
    ///    data_serialized = Vec::new();
    /// }
    /// assert_eq!(ledger_changes.0.len(), res.0.len());
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], LedgerChanges, E> {
        context(
            "Failed LedgerChanges deserialization",
            length_count(
                |input| self.length_deserializer.deserialize(input),
                tuple((
                    |input| self.address_deserializer.deserialize(input),
                    |input| self.ledger_change_deserializer.deserialize(input),
                )),
            ),
        )
        .map(|changes| LedgerChanges(changes.into_iter().collect()))
        .parse(buffer)
    }
}

impl LedgerChanges {
    /// addresses that are impacted by these ledger changes
    pub(crate) fn get_involved_addresses(&self) -> PreHashSet<Address> {
        self.0.keys().copied().collect()
    }

    /// applies a `LedgerChange`
    pub(crate) fn apply(&mut self, addr: &Address, change: &LedgerChange) -> Result<()> {
        match self.0.entry(*addr) {
            hash_map::Entry::Occupied(mut occ) => {
                occ.get_mut().chain(change)?;
                if occ.get().is_nil() {
                    occ.remove();
                }
            }
            hash_map::Entry::Vacant(vac) => {
                let mut res = LedgerChange::default();
                res.chain(change)?;
                if !res.is_nil() {
                    vac.insert(res);
                }
            }
        }
        Ok(())
    }

    /// chain with another `LedgerChange`
    pub(crate) fn chain(&mut self, other: &LedgerChanges) -> Result<()> {
        for (addr, change) in other.0.iter() {
            self.apply(addr, change)?;
        }
        Ok(())
    }

    /// merge another ledger changes into self, overwriting existing data
    /// addresses that are in not other are removed from self
    pub(crate) fn sync_from(&mut self, addrs: &PreHashSet<Address>, mut other: LedgerChanges) {
        for addr in addrs.iter() {
            if let Some(new_val) = other.0.remove(addr) {
                self.0.insert(*addr, new_val);
            } else {
                self.0.remove(addr);
            }
        }
    }

    /// clone subset
    #[must_use]
    pub(crate) fn clone_subset(&self, addrs: &PreHashSet<Address>) -> Self {
        LedgerChanges(
            self.0
                .iter()
                .filter_map(|(a, dta)| {
                    if addrs.contains(a) {
                        Some((*a, dta.clone()))
                    } else {
                        None
                    }
                })
                .collect(),
        )
    }
}
