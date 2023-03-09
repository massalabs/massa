// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::ModelsError;
use massa_serialization::{Deserializer, Serializer, U32VarIntDeserializer, U32VarIntSerializer};
use nom::bytes::complete::take;
use nom::error::context;
use nom::sequence::tuple;
use nom::Parser;
use nom::{
    error::{ContextError, ParseError},
    IResult,
};
use serde::de::Unexpected;
use std::ops::Bound::Included;
use std::{convert::TryInto, fmt, str::FromStr};

const INSTANCE_LEN: usize = 4;

/// Application version, checked during handshakes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    /// ASCII uppercase alpha
    instance: [char; INSTANCE_LEN],
    major: u32,
    minor: u32,
}

struct VersionVisitor;

impl<'de> serde::de::Visitor<'de> for VersionVisitor {
    type Value = Version;

    fn visit_str<E>(self, value: &str) -> Result<Version, E>
    where
        E: serde::de::Error,
    {
        Version::from_str(value).map_err(|_| E::invalid_value(Unexpected::Str(value), &self))
    }

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(
            formatter,
            "an Version type representing a version identifier"
        )
    }
}

impl<'de> serde::Deserialize<'de> for Version {
    fn deserialize<D>(deserializer: D) -> Result<Version, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        deserializer.deserialize_str(VersionVisitor)
    }
}

impl serde::Serialize for Version {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

/// Serializer for `Version`
pub struct VersionSerializer {
    u32_serializer: U32VarIntSerializer,
}

impl VersionSerializer {
    /// Creates a `VersionSerializer`
    pub const fn new() -> Self {
        Self {
            u32_serializer: U32VarIntSerializer::new(),
        }
    }
}

impl Default for VersionSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<Version> for VersionSerializer {
    /// ```
    /// use std::ops::Bound::Included;
    /// use std::str::FromStr;
    /// use massa_serialization::Serializer;
    /// use massa_models::version::{Version, VersionSerializer};
    ///
    /// let version: Version = Version::from_str("TEST.1.10").unwrap();
    /// let serializer = VersionSerializer::new();
    /// let mut buffer = Vec::new();
    /// serializer.serialize(&version, &mut buffer).unwrap();
    /// ```
    fn serialize(
        &self,
        value: &Version,
        buffer: &mut Vec<u8>,
    ) -> Result<(), massa_serialization::SerializeError> {
        buffer.extend(value.instance.iter().map(|&c| c as u8));
        self.u32_serializer.serialize(&value.major, buffer)?;
        self.u32_serializer.serialize(&value.minor, buffer)?;
        Ok(())
    }
}

/// Serializer for `Version`
pub struct VersionDeserializer {
    u32_deserializer: U32VarIntDeserializer,
}

impl VersionDeserializer {
    /// Creates a `VersionSerializer`
    pub const fn new() -> Self {
        Self {
            u32_deserializer: U32VarIntDeserializer::new(Included(0), Included(u32::MAX)),
        }
    }
}

impl Default for VersionDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<Version> for VersionDeserializer {
    /// ```
    /// use std::ops::Bound::Included;
    /// use std::str::FromStr;
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use massa_models::version::{Version, VersionSerializer, VersionDeserializer};
    ///
    /// let version: Version = Version::from_str("TEST.1.10").unwrap();
    /// let mut serialized = Vec::new();
    /// let serializer = VersionSerializer::new();
    /// let deserializer = VersionDeserializer::new();
    /// serializer.serialize(&version, &mut serialized).unwrap();
    /// let (rest, version_deser) = deserializer.deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert_eq!(rest.len(), 0);
    /// assert_eq!(version, version_deser);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Version, E> {
        context(
            "Failed Version deserialization",
            tuple((
                context("Failed instance deserialization", |input: &'a [u8]| {
                    let (rest, instance) = take(INSTANCE_LEN)(input)?;
                    if instance.iter().any(|c| {
                        !c.is_ascii() || !c.is_ascii_alphabetic() || !c.is_ascii_uppercase()
                    }) {
                        return Err(nom::Err::Error(ParseError::from_error_kind(
                            input,
                            nom::error::ErrorKind::Char,
                        )));
                    }
                    // Safe: because `take` fail if there is not enough data
                    let instance: [char; INSTANCE_LEN] = instance
                        .iter()
                        .map(|&c| char::from(c))
                        .collect::<Vec<char>>()
                        .try_into()
                        .unwrap();
                    Ok((rest, instance))
                }),
                context("Failed major deserialization", |input: &'a [u8]| {
                    self.u32_deserializer.deserialize(input)
                }),
                context("Failed minor deserialization", |input: &'a [u8]| {
                    self.u32_deserializer.deserialize(input)
                }),
            )),
        )
        .map(|(instance, major, minor)| Version {
            instance,
            major,
            minor,
        })
        .parse(buffer)
    }
}

impl Version {
    /// true if instance and major are the same
    pub fn is_compatible(&self, other: &Version) -> bool {
        self.instance == other.instance && self.major == other.major && other.minor >= 1
    }
}

impl fmt::Display for Version {
    /// ```rust
    /// # use massa_models::*;
    /// # use std::str::FromStr;
    /// let v: version::Version = version::Version::from_str("TEST.1.10").unwrap();
    /// assert_eq!(v.to_string(), "TEST.1.10");
    /// ```
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let network_str: String = self.instance.iter().cloned().collect();
        write!(f, "{}.{}.{}", network_str, self.major, self.minor)
    }
}

impl FromStr for Version {
    type Err = ModelsError;

    fn from_str(str_version: &str) -> Result<Self, Self::Err> {
        let parts: Vec<String> = str_version.split('.').map(|v| v.to_string()).collect();
        if parts.len() != 3 {
            return Err(ModelsError::InvalidVersionError(
                "version identifier is not in 3 parts separates by a dot".into(),
            ));
        }
        if parts[0].len() != 4
            || !parts[0].is_ascii()
            || parts[0]
                .chars()
                .any(|c| !c.is_ascii_alphabetic() || !c.is_ascii_uppercase())
        {
            return Err(ModelsError::InvalidVersionError(
                "version identifier instance part is not 4-char uppercase alphabetic ASCII".into(),
            ));
        }
        let instance: [char; 4] = parts[0].chars().collect::<Vec<char>>().try_into().unwrap(); // will not panic as the length and type were checked above
        let major: u32 = u32::from_str(&parts[1]).map_err(|_| {
            ModelsError::InvalidVersionError("version identifier major number invalid".into())
        })?;
        let minor: u32 = u32::from_str(&parts[2]).map_err(|_| {
            ModelsError::InvalidVersionError("version identifier minor number invalid".into())
        })?;
        Ok(Version {
            instance,
            major,
            minor,
        })
    }
}
