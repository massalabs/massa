// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::{
    address::Address,
    amount::{Amount, AmountDeserializer, AmountSerializer},
    error::ModelsError,
    error::ModelsResult as Result,
    prehash::{PreHashMap, PreHashSet},
};
use massa_serialization::{Deserializer, SerializeError, Serializer};
use nom::{
    error::{context, ContextError, ParseError},
    IResult, Parser,
};
use serde::{Deserialize, Serialize};
use std::{collections::hash_map, ops::Bound::Included};

/// a consensus ledger entry
#[derive(Debug, Default, Deserialize, Clone, Copy, Serialize)]
pub struct LedgerData {
    /// the balance in coins
    pub balance: Amount,
}

/// Basic serializer for `LedgerData`
#[derive(Default)]
pub struct LedgerDataSerializer {
    amount_serializer: AmountSerializer,
}

impl LedgerDataSerializer {
    /// Creates a `LedgerDataSerializer`
    pub fn new() -> Self {
        Self {
            amount_serializer: AmountSerializer::new(),
        }
    }
}

impl Serializer<LedgerData> for LedgerDataSerializer {
    /// ## Example:
    /// ```rust
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
pub struct LedgerDataDeserializer {
    amount_deserializer: AmountDeserializer,
}

impl LedgerDataDeserializer {
    /// Creates a `LedgerDataDeserializer`
    pub fn new() -> Self {
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
    /// ```rust
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
    pub fn new(starting_balance: Amount) -> LedgerData {
        LedgerData {
            balance: starting_balance,
        }
    }

    /// apply a `LedgerChange` for an entry
    /// Can fail if an overflow or underflow occurs
    pub fn apply_change(&mut self, change: &LedgerChange) -> Result<()> {
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
                        "balance underflow in LedgerData::apply_change".into(),
                    )
                })?;
        }
        Ok(())
    }

    /// returns true if the balance is zero
    pub fn is_nil(&self) -> bool {
        self.balance == Amount::default()
    }
}

/// A balance change that can be applied to an address
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerChange {
    /// Amount to add or subtract
    pub balance_delta: Amount,
    /// whether to increment or decrements balance of delta
    pub balance_increment: bool,
}

impl Default for LedgerChange {
    fn default() -> Self {
        LedgerChange {
            balance_delta: Amount::default(),
            balance_increment: true,
        }
    }
}

impl LedgerChange {
    /// Applies another ledger change on top of self
    pub fn chain(&mut self, change: &LedgerChange) -> Result<(), ModelsError> {
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
    pub fn is_nil(&self) -> bool {
        self.balance_delta == Amount::default()
    }
}

/// Map an address to a `LedgerChange`
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LedgerChanges(pub PreHashMap<Address, LedgerChange>);

impl LedgerChanges {
    /// addresses that are impacted by these ledger changes
    pub fn get_involved_addresses(&self) -> PreHashSet<Address> {
        self.0.keys().copied().collect()
    }

    /// applies a `LedgerChange`
    pub fn apply(&mut self, addr: &Address, change: &LedgerChange) -> Result<()> {
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
    pub fn chain(&mut self, other: &LedgerChanges) -> Result<()> {
        // We avoid mutating self directly to ensure atomicity in error cases.
        let mut updated = self.clone();
        for (addr, change) in other.0.iter() {
            updated.apply(addr, change)?;
        }
        *self = updated;
        Ok(())
    }

    /// merge another ledger changes into self, overwriting existing data
    /// addresses that are in not other are removed from self
    pub fn sync_from(&mut self, addrs: &PreHashSet<Address>, mut other: LedgerChanges) {
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
    pub fn clone_subset(&self, addrs: &PreHashSet<Address>) -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_change_reports_balance_underflow() {
        let mut ledger_data = LedgerData::new(Amount::zero());
        let change = LedgerChange {
            balance_delta: Amount::from_raw(1),
            balance_increment: false,
        };

        let error = ledger_data.apply_change(&change).unwrap_err();

        assert!(matches!(
            error,
            ModelsError::InvalidLedgerChange(message)
                if message == "balance underflow in LedgerData::apply_change"
        ));
    }
}
