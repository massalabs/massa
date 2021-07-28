use std::{convert::TryFrom, iter::FromIterator};

use crate::{DeserializeCompact, ModelsError, SerializeCompact, SerializeVarInt};
use serde::Serialize;

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
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        todo!()
    }
}

impl TryFrom<String> for Version {
    type Error = ModelsError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        todo!()
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
    pub fn to_string(&self) -> String {
        let mut res = String::from_iter(self.network);
        res.push('.');
        res.push_str(&self.major.to_string());
        res.push('.');
        res.push_str(&self.minor.to_string());
        res
    }
}
