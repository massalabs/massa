// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Unsigned time management
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#![feature(bound_map)]

mod error;
pub use error::TimeError;
use massa_serialization::{Deserializer, Serializer, U64VarIntDeserializer, U64VarIntSerializer};
use nom::error::{context, ContextError, ParseError};
use nom::IResult;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Bound;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{
    convert::{TryFrom, TryInto},
    str::FromStr,
};
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};

/// Time structure used everywhere.
/// milliseconds since 01/01/1970.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MassaTime(u64);

/// Serializer for `MassaTime`
pub struct MassaTimeSerializer {
    u64_serializer: U64VarIntSerializer,
}

impl MassaTimeSerializer {
    /// Creates a `MassaTimeSerializer`
    pub fn new() -> Self {
        Self {
            u64_serializer: U64VarIntSerializer::new(),
        }
    }
}

impl Default for MassaTimeSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<MassaTime> for MassaTimeSerializer {
    /// ```
    /// use std::ops::Bound::Included;
    /// use massa_serialization::Serializer;
    /// use massa_time::{MassaTime, MassaTimeSerializer};
    ///
    /// let time: MassaTime = MassaTime::from_millis(30);
    /// let mut serialized = Vec::new();
    /// let serializer = MassaTimeSerializer::new();
    /// serializer.serialize(&time, &mut serialized).unwrap();
    /// ```
    fn serialize(
        &self,
        value: &MassaTime,
        buffer: &mut Vec<u8>,
    ) -> Result<(), massa_serialization::SerializeError> {
        self.u64_serializer.serialize(&value.to_millis(), buffer)
    }
}

/// Deserializer for `MassaTime`
pub struct MassaTimeDeserializer {
    u64_deserializer: U64VarIntDeserializer,
}

impl MassaTimeDeserializer {
    /// Creates a `MassaTimeDeserializer`
    ///
    /// Arguments:
    /// * range: minimum value for the time to deserialize
    pub fn new(range: (Bound<MassaTime>, Bound<MassaTime>)) -> Self {
        Self {
            u64_deserializer: U64VarIntDeserializer::new(
                range.0.map(|time| time.to_millis()),
                range.1.map(|time| time.to_millis()),
            ),
        }
    }
}

impl Deserializer<MassaTime> for MassaTimeDeserializer {
    /// ```
    /// use std::ops::Bound::Included;
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use massa_time::{MassaTime, MassaTimeSerializer, MassaTimeDeserializer};
    ///
    /// let time: MassaTime = MassaTime::from_millis(30);
    /// let mut serialized = Vec::new();
    /// let serializer = MassaTimeSerializer::new();
    /// let deserializer = MassaTimeDeserializer::new((Included(MassaTime::from_millis(0)), Included(MassaTime::from_millis(u64::MAX))));
    /// serializer.serialize(&time, &mut serialized).unwrap();
    /// let (rest, time_deser) = deserializer.deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(time, time_deser);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], MassaTime, E> {
        context("Failed MassaTime deserialization", |input| {
            self.u64_deserializer
                .deserialize(input)
                .map(|(rest, res)| (rest, MassaTime::from_millis(res)))
        })(buffer)
    }
}

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
    /// let time : MassaTime = MassaTime::from_millis(42);
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

impl From<MassaTime> for Duration {
    /// Conversion from `massa_time` to duration, representing timestamp in milliseconds.
    /// ```
    /// # use std::time::Duration;
    /// # use massa_time::*;
    /// # use std::convert::Into;
    /// let duration: Duration = Duration::from_millis(42);
    /// let time : MassaTime = MassaTime::from_millis(42);
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
    /// let time : MassaTime = MassaTime::from_millis(42);
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
    /// Conversion from `u64`, representing timestamp in milliseconds.
    /// ```
    /// # use massa_time::*;
    /// let time : MassaTime = MassaTime::from_millis(42);
    /// ```
    pub const fn from_millis(value: u64) -> Self {
        MassaTime(value)
    }

    /// Smallest time interval
    pub const EPSILON: MassaTime = MassaTime(1);

    /// Gets current UNIX timestamp (resolution: milliseconds).
    ///
    /// ```
    /// # use std::time::{Duration, SystemTime, UNIX_EPOCH};
    /// # use massa_time::*;
    /// # use std::convert::TryFrom;
    /// # use std::cmp::max;
    /// let now_duration : Duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    /// let now_massa_time : MassaTime = MassaTime::now().unwrap();
    /// let converted  :MassaTime = MassaTime::try_from(now_duration).unwrap();
    /// assert!(max(now_massa_time.saturating_sub(converted), converted.saturating_sub(now_massa_time)) < MassaTime::from_millis(100))
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
    /// let time : MassaTime = MassaTime::from_millis(42);
    /// let res: Duration = time.to_duration();
    /// assert_eq!(res, duration);
    /// ```
    pub fn to_duration(&self) -> Duration {
        Duration::from_millis(self.0)
    }

    /// Conversion to `u64`, representing milliseconds.
    /// ```
    /// # use massa_time::*;
    /// let time : MassaTime = MassaTime::from_millis(42);
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
    /// # use std::time::Instant;
    /// let (cur_timestamp, cur_instant): (MassaTime, Instant) = (MassaTime::now().unwrap(), Instant::now());
    /// let massa_time_instant: Instant = cur_timestamp.estimate_instant().unwrap();
    /// assert!(max(
    ///     massa_time_instant.saturating_duration_since(cur_instant),
    ///     cur_instant.saturating_duration_since(massa_time_instant)
    /// ) < std::time::Duration::from_millis(10))
    /// ```
    pub fn estimate_instant(self) -> Result<Instant, TimeError> {
        let (cur_timestamp, cur_instant) = (MassaTime::now()?, Instant::now());
        if self >= cur_timestamp {
            cur_instant.checked_add(self.saturating_sub(cur_timestamp).to_duration())
        } else {
            cur_instant.checked_sub(cur_timestamp.saturating_sub(self).to_duration())
        }
        .ok_or(TimeError::TimeOverflowError)
    }

    /// ```
    /// # use massa_time::*;
    /// let time_1 : MassaTime = MassaTime::from_millis(42);
    /// let time_2 : MassaTime = MassaTime::from_millis(7);
    /// let res : MassaTime = time_1.saturating_sub(time_2);
    /// assert_eq!(res, MassaTime::from_millis(42-7))
    /// ```
    #[must_use]
    pub fn saturating_sub(self, t: MassaTime) -> Self {
        MassaTime(self.0.saturating_sub(t.0))
    }

    /// ```
    /// # use massa_time::*;
    /// let time_1 : MassaTime = MassaTime::from_millis(42);
    /// let time_2 : MassaTime = MassaTime::from_millis(7);
    /// let res : MassaTime = time_1.saturating_add(time_2);
    /// assert_eq!(res, MassaTime::from_millis(42+7))
    /// ```
    #[must_use]
    pub fn saturating_add(self, t: MassaTime) -> Self {
        MassaTime(self.0.saturating_add(t.0))
    }

    /// ```
    /// # use massa_time::*;
    /// let time_1 : MassaTime = MassaTime::from_millis(42);
    /// let time_2 : MassaTime = MassaTime::from_millis(7);
    /// let res : MassaTime = time_1.checked_sub(time_2).unwrap();
    /// assert_eq!(res, MassaTime::from_millis(42-7))
    /// ```
    pub fn checked_sub(self, t: MassaTime) -> Result<Self, TimeError> {
        self.0
            .checked_sub(t.0)
            .ok_or_else(|| TimeError::CheckedOperationError("subtraction error".to_string()))
            .map(MassaTime)
    }

    /// ```
    /// # use massa_time::*;
    /// let time_1 : MassaTime = MassaTime::from_millis(42);
    /// let time_2 : MassaTime = MassaTime::from_millis(7);
    /// let res : MassaTime = time_1.checked_add(time_2).unwrap();
    /// assert_eq!(res, MassaTime::from_millis(42+7))
    /// ```
    pub fn checked_add(self, t: MassaTime) -> Result<Self, TimeError> {
        self.0
            .checked_add(t.0)
            .ok_or_else(|| TimeError::CheckedOperationError("addition error".to_string()))
            .map(MassaTime)
    }

    /// ```
    /// # use massa_time::*;
    /// let time_1 : MassaTime = MassaTime::from_millis(42);
    /// let time_2 : MassaTime = MassaTime::from_millis(7);
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
    /// let time_1 : MassaTime = MassaTime::from_millis(42);
    /// let res : MassaTime = time_1.checked_div_u64(7).unwrap();
    /// assert_eq!(res,MassaTime::from_millis(42/7))
    /// ```
    pub fn checked_div_u64(self, n: u64) -> Result<MassaTime, TimeError> {
        self.0
            .checked_div(n)
            .ok_or_else(|| TimeError::CheckedOperationError("division error".to_string()))
            .map(MassaTime)
    }

    /// ```
    /// # use massa_time::*;
    /// let time_1 : MassaTime = MassaTime::from_millis(42);
    /// let res : MassaTime = time_1.saturating_mul(7);
    /// assert_eq!(res,MassaTime::from_millis(42*7))
    /// ```
    #[must_use]
    pub const fn saturating_mul(self, n: u64) -> MassaTime {
        MassaTime(self.0.saturating_mul(n))
    }

    /// ```
    /// # use massa_time::*;
    /// let time_1 : MassaTime = MassaTime::from_millis(42);
    /// let res : MassaTime = time_1.checked_mul(7).unwrap();
    /// assert_eq!(res,MassaTime::from_millis(42*7))
    /// ```
    pub fn checked_mul(self, n: u64) -> Result<Self, TimeError> {
        self.0
            .checked_mul(n)
            .ok_or_else(|| TimeError::CheckedOperationError("multiplication error".to_string()))
            .map(MassaTime)
    }

    /// ```
    /// # use massa_time::*;
    /// let time_1 : MassaTime = MassaTime::from_millis(42);
    /// let time_2 : MassaTime = MassaTime::from_millis(7);
    /// let res : MassaTime = time_1.checked_rem_time(time_2).unwrap();
    /// assert_eq!(res,MassaTime::from_millis(42%7))
    /// ```
    pub fn checked_rem_time(self, t: MassaTime) -> Result<Self, TimeError> {
        self.0
            .checked_rem(t.0)
            .ok_or_else(|| TimeError::CheckedOperationError("remainder error".to_string()))
            .map(MassaTime)
    }

    /// ```
    /// # use massa_time::*;
    /// let time_1 : MassaTime = MassaTime::from_millis(42);
    /// let res : MassaTime = time_1.checked_rem_u64(7).unwrap();
    /// assert_eq!(res,MassaTime::from_millis(42%7))
    /// ```
    pub fn checked_rem_u64(self, n: u64) -> Result<Self, TimeError> {
        self.0
            .checked_rem(n)
            .ok_or_else(|| TimeError::CheckedOperationError("remainder error".to_string()))
            .map(MassaTime)
    }

    /// ```
    /// # use massa_time::*;
    ///
    /// let time1 = MassaTime::from_millis(42);
    /// let time2 = MassaTime::from_millis(84);
    ///
    /// assert_eq!(time1.abs_diff(time2), MassaTime::from_millis(42));
    /// assert_eq!(time2.abs_diff(time1), MassaTime::from_millis(42));
    /// ```
    pub fn abs_diff(&self, t: MassaTime) -> MassaTime {
        MassaTime(self.0.abs_diff(t.0))
    }

    /// ```
    /// # use massa_time::*;
    /// let massa_time : MassaTime = MassaTime::from_millis(1_640_995_200_000);
    /// assert_eq!(massa_time.format_instant(), String::from("2022-01-01T00:00:00Z"))
    /// ```
    pub fn format_instant(&self) -> String {
        let naive = OffsetDateTime::from_unix_timestamp((self.to_millis() / 1000) as i64).unwrap();
        naive.format(&Rfc3339).unwrap()
    }

    /// ```
    /// # use massa_time::*;
    /// let massa_time : MassaTime = MassaTime::from_millis(1000*( 8 * 24*60*60 + 1 * 60*60 + 3 * 60 + 6 ));
    /// assert_eq!(massa_time.format_duration().unwrap(), String::from("8 days, 1 hours, 3 minutes, 6 seconds"))
    /// ```
    pub fn format_duration(&self) -> Result<String, TimeError> {
        let (days, hours, mins, secs) = self.days_hours_mins_secs()?;
        Ok(format!(
            "{} days, {} hours, {} minutes, {} seconds",
            days, hours, mins, secs
        ))
    }

    /// ```
    /// # use massa_time::*;
    /// let massa_time : MassaTime = MassaTime::from_utc_ymd_hms(2022, 2, 5, 22, 50, 40).unwrap();
    /// assert_eq!(massa_time.format_instant(), String::from("2022-02-05T22:50:40Z"))
    /// ```
    pub fn from_utc_ymd_hms(
        year: i32,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
    ) -> Result<MassaTime, TimeError> {
        let month = month.try_into().map_err(|_| TimeError::ConversionError)?;

        let date =
            Date::from_calendar_date(year, month, day).map_err(|_| TimeError::ConversionError)?;

        let date_time = date
            .with_hms(hour, minute, second)
            .map_err(|_| TimeError::ConversionError)?
            .assume_utc();

        Ok(MassaTime::from_millis(
            date_time
                .unix_timestamp_nanos()
                .checked_div(1_000_000)
                .ok_or(TimeError::ConversionError)? as u64,
        ))
    }

    /// ```
    /// # use massa_time::*;
    /// let massa_time = MassaTime::from_millis(1000 * ( 8 * 24*60*60 + 1 * 60*60 + 3 * 60 + 6 ));
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

    /// Get max MassaTime value
    pub fn max() -> MassaTime {
        MassaTime::from_millis(u64::MAX)
    }
}
