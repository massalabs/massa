// Copyright (c) 2022 MASSA LABS <info@massa.net>

mod error;
pub use error::TimeError;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{
    convert::{TryFrom, TryInto},
    str::FromStr,
};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio::time::Instant;

/// Time structure used every where.
/// Millis since 01/01/1970.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MassaTime(u64);

impl fmt::Display for MassaTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_millis())
    }
}

impl TryFrom<Duration> for MassaTime {
    type Error = TimeError;

    /// Conversion from `std::time::Duration`.
    /// ```
    /// # use std::time::Duration;
    /// # use massa_time::*;
    /// # use std::convert::TryFrom;
    /// let duration: Duration = Duration::from_millis(42);
    /// let time : MassaTime = MassaTime::from(42);
    /// assert_eq!(time, MassaTime::try_from(duration).unwrap());
    /// ```
    fn try_from(value: Duration) -> Result<Self, Self::Error> {
        Ok(MassaTime(
            value
                .as_millis()
                .try_into()
                .map_err(|_| TimeError::ConversionError)?,
        ))
    }
}

impl From<u64> for MassaTime {
    /// Conversion from u64, representing timestamp in millis.
    /// ```
    /// # use massa_time::*;
    /// let time : MassaTime = MassaTime::from(42);
    /// ```
    fn from(val: u64) -> Self {
        MassaTime(val)
    }
}

impl From<MassaTime> for Duration {
    /// Conversion massa_time to duration, representing timestamp in millis.
    /// ```
    /// # use std::time::Duration;
    /// # use massa_time::*;
    /// # use std::convert::Into;
    /// let duration: Duration = Duration::from_millis(42);
    /// let time : MassaTime = MassaTime::from(42);
    /// let res: Duration = time.into();
    /// assert_eq!(res, duration);
    /// ```
    fn from(value: MassaTime) -> Self {
        value.to_duration()
    }
}

impl FromStr for MassaTime {
    type Err = crate::TimeError;

    /// Conversion from `&str`.
    ///
    /// ```
    /// # use massa_time::*;
    /// # use std::str::FromStr;
    /// let duration: &str = "42";
    /// let time : MassaTime = MassaTime::from(42);
    ///
    /// assert_eq!(time, MassaTime::from_str(duration).unwrap());
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(MassaTime(
            u64::from_str(s).map_err(|_| Self::Err::ConversionError)?,
        ))
    }
}

impl MassaTime {
    /// Conversion from u64, representing timestamp in millis.
    /// ```
    /// # use massa_time::*;
    /// let time : MassaTime = MassaTime::from(42);
    /// ```
    pub const fn from(value: u64) -> Self {
        MassaTime(value)
    }

    /// Smallest time interval
    pub const EPSILON: MassaTime = MassaTime(1);

    /// Gets current compensated unix timestamp (resolution: milliseconds).
    ///
    /// # Parameters
    ///   * compensation_millis: when the system clock is slightly off, this parameter allows correcting it by adding this signed number of milliseconds to the locally measured timestamp
    ///
    /// ```
    /// # use std::time::{Duration, SystemTime, UNIX_EPOCH};
    /// # use massa_time::*;
    /// # use std::convert::TryFrom;
    /// # use std::cmp::max;
    /// let now_duration : Duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    /// let now_massa_time : MassaTime = MassaTime::now().unwrap();
    /// let converted  :MassaTime = MassaTime::try_from(now_duration).unwrap();
    /// assert!(max(now_massa_time.saturating_sub(converted), converted.saturating_sub(now_massa_time)) < 100.into())
    /// ```
    pub fn compensated_now(compensation_millis: i64) -> Result<Self, TimeError> {
        let now: i64 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| TimeError::TimeOverflowError)?
            .as_millis()
            .try_into()
            .map_err(|_| TimeError::TimeOverflowError)?;
        let compensated = now
            .checked_add(compensation_millis)
            .ok_or(TimeError::TimeOverflowError)?
            .try_into()
            .map_err(|_| TimeError::TimeOverflowError)?;
        Ok(MassaTime(compensated))
    }

    /// Gets current unix timestamp (resolution: milliseconds).
    ///
    /// ```
    /// # use std::time::{Duration, SystemTime, UNIX_EPOCH};
    /// # use massa_time::*;
    /// # use std::convert::TryFrom;
    /// # use std::cmp::max;
    /// let now_duration : Duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    /// let now_time : MassaTime = MassaTime::now().unwrap();
    /// let converted : MassaTime = MassaTime::try_from(now_duration).unwrap();
    /// assert!(max(now_time.saturating_sub(converted), converted.saturating_sub(now_time)) < 100.into())
    /// ```
    pub fn now() -> Result<Self, TimeError> {
        let now: u64 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| TimeError::TimeOverflowError)?
            .as_millis()
            .try_into()
            .map_err(|_| TimeError::TimeOverflowError)?;
        Ok(MassaTime(now))
    }

    /// Conversion to `std::time::Duration`.
    /// ```
    /// # use std::time::Duration;
    /// # use massa_time::*;
    /// let duration: Duration = Duration::from_millis(42);
    /// let time : MassaTime = MassaTime::from(42);
    /// let res: Duration = time.to_duration();
    /// assert_eq!(res, duration);
    /// ```
    pub fn to_duration(&self) -> Duration {
        Duration::from_millis(self.0)
    }

    /// Conversion to u64, representing millis.
    /// ```
    /// # use massa_time::*;
    /// let time : MassaTime = MassaTime::from(42);
    /// let res: u64 = time.to_millis();
    /// assert_eq!(res, 42);
    /// ```
    pub const fn to_millis(&self) -> u64 {
        self.0
    }

    /// ```
    /// # use std::time::{Duration, SystemTime, UNIX_EPOCH};
    /// # use massa_time::*;
    /// # use std::convert::TryFrom;
    /// # use std::cmp::max;
    /// # use tokio::time::Instant;
    /// let (cur_timestamp, cur_instant): (MassaTime, Instant) = (MassaTime::now().unwrap(), Instant::now());
    /// let massa_time_instant: Instant = cur_timestamp.estimate_instant(0).unwrap();
    /// assert!(max(
    ///     massa_time_instant.saturating_duration_since(cur_instant),
    ///     cur_instant.saturating_duration_since(massa_time_instant)
    /// ) < std::time::Duration::from_millis(10))
    /// ```
    pub fn estimate_instant(self, compensation_millis: i64) -> Result<Instant, TimeError> {
        let (cur_timestamp, cur_instant): (MassaTime, Instant) = (
            MassaTime::compensated_now(compensation_millis)?,
            Instant::now(),
        );
        cur_instant
            .checked_add(self.to_duration())
            .ok_or(TimeError::TimeOverflowError)?
            .checked_sub(cur_timestamp.to_duration())
            .ok_or(TimeError::TimeOverflowError)
    }

    /// ```
    /// # use massa_time::*;
    /// let time_1 : MassaTime = MassaTime::from(42);
    /// let time_2 : MassaTime = MassaTime::from(7);
    /// let res : MassaTime = time_1.saturating_sub(time_2);
    /// assert_eq!(res, MassaTime::from(42-7))
    /// ```
    #[must_use]
    pub fn saturating_sub(self, t: MassaTime) -> Self {
        MassaTime(self.0.saturating_sub(t.0))
    }

    /// ```
    /// # use massa_time::*;
    /// let time_1 : MassaTime = MassaTime::from(42);
    /// let time_2 : MassaTime = MassaTime::from(7);
    /// let res : MassaTime = time_1.saturating_add(time_2);
    /// assert_eq!(res, MassaTime::from(42+7))
    /// ```
    #[must_use]
    pub fn saturating_add(self, t: MassaTime) -> Self {
        MassaTime(self.0.saturating_add(t.0))
    }

    /// ```
    /// # use massa_time::*;
    /// let time_1 : MassaTime = MassaTime::from(42);
    /// let time_2 : MassaTime = MassaTime::from(7);
    /// let res : MassaTime = time_1.checked_sub(time_2).unwrap();
    /// assert_eq!(res, MassaTime::from(42-7))
    /// ```
    pub fn checked_sub(self, t: MassaTime) -> Result<Self, TimeError> {
        self.0
            .checked_sub(t.0)
            .ok_or_else(|| TimeError::CheckedOperationError("subtraction error".to_string()))
            .map(MassaTime)
    }

    /// ```
    /// # use massa_time::*;
    /// let time_1 : MassaTime = MassaTime::from(42);
    /// let time_2 : MassaTime = MassaTime::from(7);
    /// let res : MassaTime = time_1.checked_add(time_2).unwrap();
    /// assert_eq!(res, MassaTime::from(42+7))
    /// ```
    pub fn checked_add(self, t: MassaTime) -> Result<Self, TimeError> {
        self.0
            .checked_add(t.0)
            .ok_or_else(|| TimeError::CheckedOperationError("addition error".to_string()))
            .map(MassaTime)
    }

    /// ```
    /// # use massa_time::*;
    /// let time_1 : MassaTime = MassaTime::from(42);
    /// let time_2 : MassaTime = MassaTime::from(7);
    /// let res : u64 = time_1.checked_div_time(time_2).unwrap();
    /// assert_eq!(res,42/7)
    /// ```
    pub fn checked_div_time(self, t: MassaTime) -> Result<u64, TimeError> {
        self.0
            .checked_div(t.0)
            .ok_or_else(|| TimeError::CheckedOperationError("division error".to_string()))
    }

    /// ```
    /// # use massa_time::*;
    /// let time_1 : MassaTime = MassaTime::from(42);
    /// let res : MassaTime = time_1.checked_div_u64(7).unwrap();
    /// assert_eq!(res,MassaTime::from(42/7))
    /// ```
    pub fn checked_div_u64(self, n: u64) -> Result<MassaTime, TimeError> {
        self.0
            .checked_div(n)
            .ok_or_else(|| TimeError::CheckedOperationError("division error".to_string()))
            .map(MassaTime)
    }

    /// ```
    /// # use massa_time::*;
    /// let time_1 : MassaTime = MassaTime::from(42);
    /// let res : MassaTime = time_1.saturating_mul(7);
    /// assert_eq!(res,MassaTime::from(42*7))
    /// ```
    #[must_use]
    pub fn saturating_mul(self, n: u64) -> MassaTime {
        MassaTime(self.0.saturating_mul(n))
    }

    /// ```
    /// # use massa_time::*;
    /// let time_1 : MassaTime = MassaTime::from(42);
    /// let res : MassaTime = time_1.checked_mul(7).unwrap();
    /// assert_eq!(res,MassaTime::from(42*7))
    /// ```
    pub fn checked_mul(self, n: u64) -> Result<Self, TimeError> {
        self.0
            .checked_mul(n)
            .ok_or_else(|| TimeError::CheckedOperationError("multiplication error".to_string()))
            .map(MassaTime)
    }

    /// ```
    /// # use massa_time::*;
    /// let time_1 : MassaTime = MassaTime::from(42);
    /// let time_2 : MassaTime = MassaTime::from(7);
    /// let res : MassaTime = time_1.checked_rem_time(time_2).unwrap();
    /// assert_eq!(res,MassaTime::from(42%7))
    /// ```
    pub fn checked_rem_time(self, t: MassaTime) -> Result<Self, TimeError> {
        self.0
            .checked_rem(t.0)
            .ok_or_else(|| TimeError::CheckedOperationError("remainder error".to_string()))
            .map(MassaTime)
    }

    /// ```
    /// # use massa_time::*;
    /// let time_1 : MassaTime = MassaTime::from(42);
    /// let res : MassaTime = time_1.checked_rem_u64(7).unwrap();
    /// assert_eq!(res,MassaTime::from(42%7))
    /// ```
    pub fn checked_rem_u64(self, n: u64) -> Result<Self, TimeError> {
        self.0
            .checked_rem(n)
            .ok_or_else(|| TimeError::CheckedOperationError("remainder error".to_string()))
            .map(MassaTime)
    }

    /// ```
    /// # use massa_time::*;
    /// let massa_time : MassaTime = MassaTime::from(1640995200);
    /// assert_eq!(massa_time.to_utc_string(), "2022-01-01T00:00:00Z")
    /// ```
    pub fn to_utc_string(self) -> String {
        let naive = OffsetDateTime::from_unix_timestamp((self.to_millis() / 1000) as i64).unwrap();
        naive.format(&Rfc3339).unwrap()
    }

    /// ```
    /// # use massa_time::*;
    /// let massa_time = MassaTime::from(1000 * ( 8 * 24*60*60 + 1 * 60*60 + 3 * 60 + 6 ));
    /// let (days, hours, mins, secs) = massa_time.days_hours_mins_secs().unwrap();
    /// assert_eq!(days, 8);
    /// assert_eq!(hours, 1);
    /// assert_eq!(mins, 3);
    /// assert_eq!(secs, 6);
    /// ```
    pub fn days_hours_mins_secs(&self) -> Result<(i64, i64, i64, i64), TimeError> {
        let time: time::Duration = time::Duration::try_from(self.to_duration())
            .map_err(|_| TimeError::TimeOverflowError)?;
        let days = time.whole_days();
        let hours = (time - time::Duration::days(days)).whole_hours();
        let mins =
            (time - time::Duration::days(days) - time::Duration::hours(hours)).whole_minutes();
        let secs = (time
            - time::Duration::days(days)
            - time::Duration::hours(hours)
            - time::Duration::minutes(mins))
        .whole_seconds();
        Ok((days, hours, mins, secs))
    }
}
