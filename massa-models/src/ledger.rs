// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::{u8_from_slice, Amount, DeserializeCompact, ModelsError, SerializeCompact};
use core::usize;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct LedgerData {
    pub balance: Amount,
}

impl Default for LedgerData {
    fn default() -> Self {
        LedgerData {
            balance: Amount::default(),
        }
    }
}

/// Checks performed:
/// - Balance.
impl SerializeCompact for LedgerData {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, crate::ModelsError> {
        let mut res: Vec<u8> = Vec::new();
        res.extend(&self.balance.to_bytes_compact()?);
        Ok(res)
    }
}

/// Checks performed:
/// - Balance.
impl DeserializeCompact for LedgerData {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), crate::ModelsError> {
        let mut cursor = 0usize;
        let (balance, delta) = Amount::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;
        Ok((LedgerData { balance }, cursor))
    }
}

impl LedgerData {
    pub fn new(starting_balance: Amount) -> LedgerData {
        LedgerData {
            balance: starting_balance,
        }
    }

    pub fn apply_change(&mut self, change: &LedgerChange) -> Result<(), ModelsError> {
        if change.balance_increment {
            self.balance = self.balance.checked_add(change.balance_delta).ok_or(
                ModelsError::InvalidLedgerChange(
                    "balance overflow in LedgerData::apply_change".into(),
                ),
            )?;
        } else {
            self.balance = self.balance.checked_sub(change.balance_delta).ok_or(
                ModelsError::InvalidLedgerChange(
                    "balance underflow in LedgerData::apply_change".into(),
                ),
            )?;
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
    /// Amount to add or substract
    pub balance_delta: Amount,
    /// whether to increment or decrement balance of delta
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
            self.balance_delta = self.balance_delta.checked_add(change.balance_delta).ok_or(
                ModelsError::InvalidLedgerChange("overflow in LedgerChange::chain".into()),
            )?;
        } else if change.balance_delta > self.balance_delta {
            self.balance_delta = change.balance_delta.checked_sub(self.balance_delta).ok_or(
                ModelsError::InvalidLedgerChange("underflow in LedgerChange::chain".into()),
            )?;
            self.balance_increment = !self.balance_increment;
        } else {
            self.balance_delta = self.balance_delta.checked_sub(change.balance_delta).ok_or(
                ModelsError::InvalidLedgerChange("underflow in LedgerChange::chain".into()),
            )?;
        }
        if self.balance_delta == Amount::default() {
            self.balance_increment = true;
        }
        Ok(())
    }

    pub fn is_nil(&self) -> bool {
        self.balance_delta == Amount::default()
    }
}

/// Checks performed:
/// - Balance delta.
impl SerializeCompact for LedgerChange {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, crate::ModelsError> {
        let mut res: Vec<u8> = Vec::new();
        res.push(if self.balance_increment { 1u8 } else { 0u8 });
        res.extend(&self.balance_delta.to_bytes_compact()?);
        Ok(res)
    }
}

/// Checks performed:
/// - Increment flag.
/// - Balance delta.
impl DeserializeCompact for LedgerChange {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), crate::ModelsError> {
        let mut cursor = 0usize;

        let balance_increment = match u8_from_slice(&buffer[cursor..])? {
            0u8 => false,
            1u8 => true,
            _ => {
                return Err(ModelsError::DeserializeError(
                    "wrong boolean balance_increment encoding in LedgerChange deserialization"
                        .into(),
                ))
            }
        };
        cursor += 1;

        let (balance_delta, delta) = Amount::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        Ok((
            LedgerChange {
                balance_increment,
                balance_delta,
            },
            cursor,
        ))
    }
}
