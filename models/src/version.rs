use std::{
    convert::{TryFrom, TryInto},
    iter::FromIterator,
};

use crate::serialization::DeserializeVarInt;
use crate::{DeserializeCompact, ModelsError, SerializeCompact, SerializeVarInt};
use serde::Serialize;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Version {
    pub network: [char; 4], // ascii and alpha (maj only)
    pub major: u32,
    pub minor: u32,
}

impl Default for Version {
    fn default() -> Self {
        Version {
            network: ['O', 'O', 'O', 'O'],
            major: 0,
            minor: 0,
        }
    }
}

impl<'de> ::serde::Deserialize<'de> for Version {
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        todo!()
    }
}

impl SerializeCompact for Version {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();
        res.extend(self.network.iter().map(|&c| {
            let mut dst = [0; 1]; //should be enough
            c.encode_utf8(&mut dst);
            dst[0]
        }));
        res.extend(self.major.to_varint_bytes());
        res.extend(self.minor.to_varint_bytes());
        Ok(res)
    }
}

impl DeserializeCompact for Version {
    /// ```
    /// # use models::*;
    /// # use std::convert::TryFrom;
    /// let v: Version = Version {
    ///    network: ['T', 'E', 'S', 'T'],
    ///    major: 1,
    ///    minor: 2,
    /// };
    /// let ser = v.to_bytes_compact().unwrap();
    /// let (deser, _) = Version::from_bytes_compact(&ser).unwrap();
    /// assert_eq!(deser, v)
    /// ```
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0;
        let network = buffer[0..4]
            .iter()
            .map(|c| char::from(*c))
            .collect::<Vec<_>>()
            .get(..4)
            .ok_or_else(|| {
                ModelsError::DeserializeError(format!("error deserialising version network ",))
            })?
            .try_into()
            .map_err(|e| {
                ModelsError::DeserializeError(format!(
                    "error deserialising version network {:?} ",
                    e
                ))
            })?;
        cursor += 4;

        let (major, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        let (minor, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        Ok((
            Version {
                network,
                major,
                minor,
            },
            cursor,
        ))
    }
}

impl TryFrom<String> for Version {
    type Error = ModelsError;

    /// ```
    /// # use models::*;
    /// # use std::convert::TryFrom;
    /// let v: Version = Version {
    ///    network: ['T', 'E', 'S', 'T'],
    ///    major: 1,
    ///    minor: 2,
    /// };
    /// assert_eq!(Version::try_from("TEST.1.2".to_string()).unwrap(), v)
    /// ```
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let val = value.split('.').collect::<Vec<_>>();
        if val.len() != 3 {
            return Err(ModelsError::DeserializeError(
                "Wrong version format".to_string(),
            ));
        }
        let network = val[0]
            .chars()
            .collect::<Vec<_>>()
            .get(..4)
            .ok_or_else(|| {
                ModelsError::DeserializeError(format!("error deserialising version network ",))
            })?
            .try_into()
            .map_err(|e| {
                ModelsError::DeserializeError(format!(
                    "error deserialising version network {:?} ",
                    e
                ))
            })?;
        let major = u32::from_str(val[1]).map_err(|e| {
            ModelsError::DeserializeError(format!("error deserialising version major {:?}", e))
        })?;
        let minor = u32::from_str(val[2]).map_err(|e| {
            ModelsError::DeserializeError(format!("error deserialising version minor {:?}", e))
        })?;
        Ok(Version {
            network,
            major,
            minor,
        })
    }
}

impl Version {
    pub fn is_compatible(&self, other: &Version) -> bool {
        self.network == other.network && self.major == other.major
    }

    /// ```
    /// # use models::*;
    /// let v: Version = Version {
    ///    network: ['T', 'E', 'S', 'T'],
    ///    major: 1,
    ///    minor: 2,
    /// };
    /// assert_eq!(v.to_string(), "TEST.1.2");
    /// ```
    pub fn to_string(&self) -> String {
        let mut res = String::from_iter(self.network);
        res.push('.');
        res.push_str(&self.major.to_string());
        res.push('.');
        res.push_str(&self.minor.to_string());
        res
    }
}
