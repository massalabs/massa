// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::ModelsError;
use rust_decimal::prelude::*;
use serde::de::Unexpected;
use std::fmt;
use std::str::FromStr;

pub const AMOUNT_DECIMAL_FACTOR: u64 = 1_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Ord, PartialOrd, Default)]
pub struct Amount(u64);

impl num_traits::CheckedAdd for Amount {
    fn checked_add(&self, v: &Self) -> Option<Self> {
        self.0.checked_add(v.0).map(Amount)
    }
}

impl std::ops::Add for Amount {
    type Output = Amount;
    fn add(self, rhs: Self) -> Self::Output {
        Amount(self.0 + rhs.0)
    }
}

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
    /// # use std::str::FromStr;
    /// let amount_1 : Amount = Amount::from_str("42").unwrap();
    /// let amount_2 : Amount = Amount::from_str("7").unwrap();
    /// let res : Amount = amount_1.checked_sub(amount_2).unwrap();
    /// assert_eq!(res, Amount::from_str("35").unwrap())
    /// ```
    pub fn checked_sub(self, amount: Amount) -> Option<Self> {
        self.0.checked_sub(amount.0).map(Amount)
    }

    /// ```
    /// # use models::Amount;
    /// # use std::str::FromStr;
    /// let amount_1 : Amount = Amount::from_str("42").unwrap();
    /// let amount_2 : Amount = Amount::from_str("7").unwrap();
    /// let res : Amount = amount_1.checked_add(amount_2).unwrap();
    /// assert_eq!(res, Amount::from_str("49").unwrap())
    /// ```
    pub fn checked_add(self, amount: Amount) -> Option<Self> {
        self.0.checked_add(amount.0).map(Amount)
    }

    /// ```
    /// # use models::Amount;
    /// # use std::str::FromStr;
    /// let amount_1 : Amount = Amount::from_str("42").unwrap();
    /// let res : Amount = amount_1.checked_mul_u64(7).unwrap();
    /// assert_eq!(res, Amount::from_str("294").unwrap())
    /// ```
    pub fn checked_mul_u64(self, factor: u64) -> Option<Self> {
        self.0.checked_mul(factor).map(Amount)
    }

    /// ```
    /// # use models::Amount;
    /// # use std::str::FromStr;
    /// let amount_1 : Amount = Amount::from_str("42").unwrap();
    /// let res : Amount = amount_1.checked_div_u64(7).unwrap();
    /// assert_eq!(res, Amount::from_str("6").unwrap());
    /// ```
    pub fn checked_div_u64(self, factor: u64) -> Option<Self> {
        self.0.checked_div(factor).map(Amount)
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

impl<'de> serde::Deserialize<'de> for Amount {
    fn deserialize<D>(deserializer: D) -> Result<Amount, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        deserializer.deserialize_str(AmountVisitor)
    }
}

struct AmountVisitor;

impl<'de> serde::de::Visitor<'de> for AmountVisitor {
    type Value = Amount;

    fn visit_str<E>(self, value: &str) -> Result<Amount, E>
    where
        E: serde::de::Error,
    {
        Amount::from_str(value).map_err(|_| E::invalid_value(Unexpected::Str(value), &self))
    }

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(
            formatter,
            "an Amount type representing a fixed-point currency amount"
        )
    }
}

impl serde::Serialize for Amount {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
