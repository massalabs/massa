// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::{array_from_slice, serialization::DeserializeVarInt};
use crate::{DeserializeCompact, ModelsError, SerializeCompact, SerializeVarInt};
use serde::de::Unexpected;
use std::{convert::TryInto, fmt, str::FromStr};

/// Application version, checked during handshakes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    /// ASCII uppercase alpha
    instance: [char; 4],
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

/// Checks performed: none.
impl SerializeCompact for Version {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();
        res.extend(self.instance.iter().map(|&c| c as u8));
        res.extend(self.major.to_varint_bytes());
        res.extend(self.minor.to_varint_bytes());
        Ok(res)
    }
}

/// Checks performed:
/// - Validity of instance.
/// - Validity of major version.
/// - Validity of minor version.
impl DeserializeCompact for Version {
    /// ```rust
    /// # use massa_models::*;
    /// # use std::str::FromStr;
    /// let v: Version = Version::from_str("TEST.1.2").unwrap();
    /// let ser = v.to_bytes_compact().unwrap();
    /// let (deser, _) = Version::from_bytes_compact(&ser).unwrap();
    /// assert_eq!(deser, v)
    /// ```
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0;

        // instance
        let instance: [u8; 4] = array_from_slice(&buffer[cursor..])?;
        cursor += 4;
        if instance
            .iter()
            .any(|c| !c.is_ascii() || !c.is_ascii_alphabetic() || !c.is_ascii_uppercase())
        {
            return Err(ModelsError::InvalidVersionError(
                "invalid instance value in version identifier during compact deserialization"
                    .into(),
            ));
        }
        let instance: [char; 4] = instance
            .iter()
            .map(|&c| c.into())
            .collect::<Vec<char>>()
            .try_into()
            .unwrap(); // will not panic as it was tested above

        // major
        let (major, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        // minor
        let (minor, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        Ok((
            Version {
                instance,
                major,
                minor,
            },
            cursor,
        ))
    }
}

impl Version {
    /// true if instance and major are the same
    pub fn is_compatible(&self, other: &Version) -> bool {
        // TODO for next testnet: update this control statement
        if other.major == 9 && other.minor == 0 {
            return false;
        }
        self.instance == other.instance && self.major == other.major
    }
}

impl fmt::Display for Version {
    /// ```rust
    /// # use massa_models::*;
    /// # use std::str::FromStr;
    /// let v: Version = Version::from_str("TEST.1.2").unwrap();
    /// assert_eq!(v.to_string(), "TEST.1.2");
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
