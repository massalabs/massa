mod error;
pub use error::TimeError;
use std::convert::{TryFrom, TryInto};
use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::Instant;

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UTime(u64);

impl fmt::Display for UTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_millis())
    }
}

impl From<u64> for UTime {
    fn from(value: u64) -> Self {
        UTime(value)
    }
}

impl TryFrom<Duration> for UTime {
    type Error = TimeError;

    fn try_from(value: Duration) -> Result<Self, Self::Error> {
        Ok(UTime(
            value
                .as_millis()
                .try_into()
                .map_err(|_| TimeError::ConversionError)?,
        ))
    }
}

impl Into<Duration> for UTime {
    fn into(self) -> Duration {
        Duration::from_millis(self.to_millis())
    }
}

impl UTime {
    pub fn now() -> Result<Self, TimeError> {
        Ok(UTime(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| TimeError::TimeOverflowError)?
                .as_millis()
                .try_into()
                .map_err(|_| TimeError::TimeOverflowError)?,
        ))
    }

    pub fn to_duration(&self) -> Duration {
        Duration::from_millis(self.0)
    }

    pub fn to_millis(&self) -> u64 {
        self.0
    }

    pub fn estimate_instant(self) -> Result<Instant, TimeError> {
        let (cur_timestamp, cur_instant): (UTime, Instant) = (UTime::now()?, Instant::now());
        Ok(cur_instant
            .checked_sub(cur_timestamp.to_duration())
            .ok_or(TimeError::TimeOverflowError)?
            .checked_add(self.to_duration())
            .ok_or(TimeError::TimeOverflowError)?)
    }

    pub fn saturating_sub(self, t: UTime) -> Self {
        UTime(self.0.saturating_sub(t.0))
    }

    pub fn saturating_add(self, t: UTime) -> Self {
        UTime(self.0.saturating_add(t.0))
    }

    pub fn checked_sub(self, t: UTime) -> Result<Self, TimeError> {
        self.0
            .checked_sub(t.0)
            .ok_or(TimeError::CheckedOperationError(format!(
                "substraction error"
            )))
            .and_then(|value| Ok(UTime(value)))
    }

    pub fn checked_add(self, t: UTime) -> Result<Self, TimeError> {
        self.0
            .checked_add(t.0)
            .ok_or(TimeError::CheckedOperationError(format!("addition error")))
            .and_then(|value| Ok(UTime(value)))
    }

    pub fn checked_div_time(self, t: UTime) -> Result<u64, TimeError> {
        self.0
            .checked_div(t.0)
            .ok_or(TimeError::CheckedOperationError(format!("division error")))
    }

    pub fn checked_div_u64(self, n: u64) -> Result<UTime, TimeError> {
        self.0
            .checked_div(n)
            .ok_or(TimeError::CheckedOperationError(format!("division error")))
            .and_then(|value| Ok(UTime(value)))
    }

    pub fn checked_mul(self, n: u64) -> Result<Self, TimeError> {
        self.0
            .checked_mul(n)
            .ok_or(TimeError::CheckedOperationError(format!(
                "multiplication error"
            )))
            .and_then(|value| Ok(UTime(value)))
    }

    pub fn checked_rem_time(self, t: UTime) -> Result<Self, TimeError> {
        self.0
            .checked_rem(t.0)
            .ok_or(TimeError::CheckedOperationError(format!("remainder error")))
            .and_then(|value| Ok(UTime(value)))
    }

    pub fn checked_rem_u64(self, n: u64) -> Result<Self, TimeError> {
        self.0
            .checked_rem(n)
            .ok_or(TimeError::CheckedOperationError(format!("remainder error")))
            .and_then(|value| Ok(UTime(value)))
    }
}
