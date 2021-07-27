use std::convert::TryFrom;

use crate::{DeserializeCompact, ModelsError, SerializeCompact};

pub struct Version {
    network: [char; 4], // ascii and alpha (maj only)
    major: u32,
    minor: u32,
}

impl SerializeCompact for Version {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        todo!()
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
        todo!()
    }

    pub fn to_string(&self) -> String {
        todo!()
    }
}
