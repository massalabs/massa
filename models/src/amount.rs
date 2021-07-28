use crate::ModelsError;
use rust_decimal::prelude::*;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{AddAssign, Sub, SubAssign};
use std::str::FromStr;

const AMOUNT_DECIMAL_FACTOR: u64 = 1_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Ord, PartialOrd, Deserialize)]
pub struct Amount(u64);

impl Amount {
    pub fn saturating_add(self, amount: Amount) -> Self {
        Amount(self.0.saturating_add(amount.0))
    }

    pub fn saturating_sub(self, amount: Amount) -> Self {
        Amount(self.0.saturating_sub(amount.0))
    }

    /// ```
    /// # use models::Amount;
    /// let amount_1 : Amount = Amount::from(42);
    /// let amount_2 : Amount = Amount::from(7);
    /// let res : Amount = amount_1.checked_sub(amount_2).unwrap();
    /// assert_eq!(res, Amount::from(42-7))
    /// ```
    pub fn checked_sub(self, amount: Amount) -> Result<Self, ModelsError> {
        self.0
            .checked_sub(amount.0)
            .ok_or_else(|| ModelsError::CheckedOperationError("subtraction error".to_string()))
            .map(Amount)
    }

    /// ```
    /// # use models::Amount;
    /// let amount_1 : Amount = Amount::from(42);
    /// let amount_2 : Amount = Amount::from(7);
    /// let res : Amount = amount_1.checked_add(amount_2).unwrap();
    /// assert_eq!(res, Amount::from(42+7))
    /// ```
    pub fn checked_add(self, amount: Amount) -> Result<Self, ModelsError> {
        self.0
            .checked_add(amount.0)
            .ok_or_else(|| ModelsError::CheckedOperationError("addition error".to_string()))
            .map(Amount)
    }

    /// ```
    /// # use models::Amount;
    /// let amount_1 : Amount = Amount::from(42);
    /// let res : Amount = amount_1.checked_mul(Amount::from(7)).unwrap();
    /// assert_eq!(res, Amount::from(42*7))
    /// ```
    pub fn checked_mul(self, n: Amount) -> Result<Self, ModelsError> {
        self.0
            .checked_mul(n.0)
            .ok_or_else(|| ModelsError::CheckedOperationError("multiplication error".to_string()))
            .map(Amount)
    }
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let formatted: String = self.clone().into();
        write!(f, "{}", formatted)
    }
}

impl FromStr for Amount {
    type Err = ModelsError;

    fn from_str(str_amount: &str) -> Result<Self, Self::Err> {
        let res = Decimal::from_str(str_amount)
            .map_err(|err| ModelsError::AmountParseError(err.to_string()))?
            .checked_mul(AMOUNT_DECIMAL_FACTOR.into())
            .ok_or_else(|| ModelsError::AmountParseError("amount is too large".to_string()))?;
        if res.is_sign_negative() {
            return Err(ModelsError::AmountParseError(
                "amounts should be positive".to_string(),
            ));
        }
        if !res.fract().is_zero() {
            return Err(ModelsError::AmountParseError(format!(
                "amounts should have a precision down to 1/{}",
                AMOUNT_DECIMAL_FACTOR
            )));
        }
        let res = res
            .to_u64()
            .ok_or_else(|| ModelsError::AmountParseError("amount is too large".to_string()))?;
        Ok(Amount(res))
    }
}

impl Into<String> for Amount {
    fn into(self) -> String {
        Decimal::from_u64(self.0)
            .unwrap() // will never panic
            .checked_div(AMOUNT_DECIMAL_FACTOR.into()) // will never panic
            .unwrap() // will never panic
            .to_string()
    }
}

impl From<u64> for Amount {
    fn from(amount: u64) -> Amount {
        Amount(amount)
    }
}

impl Into<u64> for Amount {
    fn into(self) -> u64 {
        self.0
    }
}

impl Into<u64> for &Amount {
    fn into(self) -> u64 {
        self.0
    }
}

impl std::cmp::PartialEq<u64> for Amount {
    fn eq(&self, other: &u64) -> bool {
        &self.0 == other
    }
}

impl Sub for Amount {
    type Output = Self;

    fn sub(self, other: Self) -> Self::Output {
        Amount(self.0 - other.0)
    }
}

impl AddAssign for Amount {
    fn add_assign(&mut self, other: Self) {
        *self = Self(self.0 + other.0);
    }
}

impl SubAssign for Amount {
    fn sub_assign(&mut self, other: Self) {
        *self = Self(self.0 - other.0);
    }
}
