use crate::ModelsError;
use rust_decimal::prelude::*;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

const AMOUNT_DECIMAL_FACTOR: u64 = 1_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Ord, PartialOrd, Deserialize, Default)]
pub struct Amount(u64);

impl Amount {
    pub fn to_raw(&self) -> u64 {
        self.0
    }

    pub fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

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
    /// assert_eq!(res, Amount::from_str("35").unwrap())
    /// ```
    pub fn checked_sub(self, amount: Amount) -> Option<Self> {
        self.0.checked_sub(amount.0).map(Amount)
    }

    /// ```
    /// # use models::Amount;
    /// let amount_1 : Amount = Amount::from(42);
    /// let amount_2 : Amount = Amount::from(7);
    /// let res : Amount = amount_1.checked_add(amount_2).unwrap();
    /// assert_eq!(res, Amount::from_str("49").unwrap())
    /// ```
    pub fn checked_add(self, amount: Amount) -> Option<Self> {
        self.0.checked_add(amount.0).map(Amount)
    }

    /// ```
    /// # use models::Amount;
    /// let amount_1 : Amount = Amount::from(42);
    /// let res : Amount = amount_1.checked_mul(Amount::from(7)).unwrap();
    /// assert_eq!(res, Amount::from_str("294").unwrap())
    /// ```
    pub fn checked_mul_u64(self, factor: u64) -> Option<Self> {
        self.0.checked_mul(factor).map(Amount)
    }
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let res_string = Decimal::from_u64(self.0)
            .unwrap() // will never panic
            .checked_div(AMOUNT_DECIMAL_FACTOR.into()) // will never panic
            .unwrap() // will never panic
            .to_string();
        write!(f, "{}", res_string)
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
                "amounts cannot be strictly negative".to_string(),
            ));
        }
        if !res.fract().is_zero() {
            return Err(ModelsError::AmountParseError(format!(
                "amounts cannot be more precise than 1/{}",
                AMOUNT_DECIMAL_FACTOR
            )));
        }
        let res = res.to_u64().ok_or_else(|| {
            ModelsError::AmountParseError(
                "amount is too large to be represented as u64".to_string(),
            )
        })?;
        Ok(Amount(res))
    }
}
