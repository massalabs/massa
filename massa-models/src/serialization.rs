// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::ModelsError;
use crate::Amount;
use integer_encoding::VarInt;
use massa_time::MassaTime;
use std::convert::TryInto;
use std::net::IpAddr;

pub trait SerializeVarInt {
    /// Serialize as varint bytes
    fn to_varint_bytes(self) -> Vec<u8>;
}

impl SerializeVarInt for u32 {
    fn to_varint_bytes(self) -> Vec<u8> {
        self.encode_var_vec()
    }
}

impl SerializeVarInt for u64 {
    fn to_varint_bytes(self) -> Vec<u8> {
        self.encode_var_vec()
    }
}

pub trait DeserializeVarInt: Sized {
    /// Deserialize variable size integer to Self from the provided buffer.
    /// The data to deserialize starts at the beginning of the buffer but the buffer can be larger than needed.
    /// In case of success, return the deserialized data and the number of bytes read
    fn from_varint_bytes(buffer: &[u8]) -> Result<(Self, usize), ModelsError>;

    /// Deserialize variable size integer to Self from the provided buffer and checks that its value is within given bounds.
    /// The data to deserialize starts at the beginning of the buffer but the buffer can be larger than needed.
    /// In case of success, return the deserialized data and the number of bytes read
    fn from_varint_bytes_bounded(
        buffer: &[u8],
        max_value: Self,
    ) -> Result<(Self, usize), ModelsError>;
}

impl DeserializeVarInt for u32 {
    fn from_varint_bytes(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        u32::decode_var(buffer)
            .ok_or_else(|| ModelsError::DeserializeError("could not deserialize varint".into()))
    }

    fn from_varint_bytes_bounded(
        buffer: &[u8],
        max_value: Self,
    ) -> Result<(Self, usize), ModelsError> {
        let (res, res_size) = Self::from_varint_bytes(buffer)?;
        if res > max_value {
            return Err(ModelsError::DeserializeError(
                "deserialized varint u32 out of bounds".into(),
            ));
        }
        Ok((res, res_size))
    }
}

impl DeserializeVarInt for u64 {
    fn from_varint_bytes(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        u64::decode_var(buffer)
            .ok_or_else(|| ModelsError::DeserializeError("could not deserialize varint".into()))
    }

    fn from_varint_bytes_bounded(
        buffer: &[u8],
        max_value: Self,
    ) -> Result<(Self, usize), ModelsError> {
        let (res, res_size) = Self::from_varint_bytes(buffer)?;
        if res > max_value {
            return Err(ModelsError::DeserializeError(
                "deserialized varint u64 out of bounds".into(),
            ));
        }
        Ok((res, res_size))
    }
}

pub trait SerializeMinBEInt {
    /// serializes with the minimal amount of big endian bytes
    fn to_be_bytes_min(self, max_value: Self) -> Result<Vec<u8>, ModelsError>;
}

impl SerializeMinBEInt for u32 {
    fn to_be_bytes_min(self, max_value: Self) -> Result<Vec<u8>, ModelsError> {
        if self > max_value {
            return Err(ModelsError::SerializeError("integer out of bounds".into()));
        }
        let skip_bytes = (max_value.leading_zeros() as usize) / 8;
        Ok(self.to_be_bytes()[skip_bytes..].to_vec())
    }
}

impl SerializeMinBEInt for u64 {
    fn to_be_bytes_min(self, max_value: Self) -> Result<Vec<u8>, ModelsError> {
        if self > max_value {
            return Err(ModelsError::SerializeError("integer out of bounds".into()));
        }
        let skip_bytes = (max_value.leading_zeros() as usize) / 8;
        Ok(self.to_be_bytes()[skip_bytes..].to_vec())
    }
}

pub trait DeserializeMinBEInt: Sized {
    const MIN_BE_INT_BASE_SIZE: usize;

    /// Compute the minimal big endian deserialization size
    fn be_bytes_min_length(max_value: Self) -> usize;

    /// Deserializes a minimally sized big endian integer to Self from the provided buffer and checks that its value is within given bounds.
    /// In case of success, return the deserialized data and the number of bytes read
    fn from_be_bytes_min(buffer: &[u8], max_value: Self) -> Result<(Self, usize), ModelsError>;
}

impl DeserializeMinBEInt for u32 {
    const MIN_BE_INT_BASE_SIZE: usize = 4;

    fn be_bytes_min_length(max_value: Self) -> usize {
        Self::MIN_BE_INT_BASE_SIZE - (max_value.leading_zeros() as usize) / 8
    }

    fn from_be_bytes_min(buffer: &[u8], max_value: Self) -> Result<(Self, usize), ModelsError> {
        let read_bytes = Self::be_bytes_min_length(max_value);
        let skip_bytes = Self::MIN_BE_INT_BASE_SIZE - read_bytes;
        if buffer.len() < read_bytes {
            return Err(ModelsError::SerializeError("unexpected buffer end".into()));
        }
        let mut buf = [0u8; Self::MIN_BE_INT_BASE_SIZE];
        buf[skip_bytes..].clone_from_slice(&buffer[..read_bytes]);
        let res = u32::from_be_bytes(buf);
        if res > max_value {
            return Err(ModelsError::SerializeError(
                "integer outside of bounds".into(),
            ));
        }
        Ok((res, read_bytes))
    }
}

impl DeserializeMinBEInt for u64 {
    const MIN_BE_INT_BASE_SIZE: usize = 8;

    fn be_bytes_min_length(max_value: Self) -> usize {
        Self::MIN_BE_INT_BASE_SIZE - (max_value.leading_zeros() as usize) / 8
    }

    fn from_be_bytes_min(buffer: &[u8], max_value: Self) -> Result<(Self, usize), ModelsError> {
        let read_bytes = Self::be_bytes_min_length(max_value);
        let skip_bytes = Self::MIN_BE_INT_BASE_SIZE - read_bytes;
        if buffer.len() < read_bytes {
            return Err(ModelsError::SerializeError("unexpected buffer end".into()));
        }
        let mut buf = [0u8; Self::MIN_BE_INT_BASE_SIZE];
        buf[skip_bytes..].clone_from_slice(&buffer[..read_bytes]);
        let res = u64::from_be_bytes(buf);
        if res > max_value {
            return Err(ModelsError::SerializeError(
                "integer outside of bounds".into(),
            ));
        }
        Ok((res, read_bytes))
    }
}

pub fn array_from_slice<const ARRAY_SIZE: usize>(
    buffer: &[u8],
) -> Result<[u8; ARRAY_SIZE], ModelsError> {
    if buffer.len() < ARRAY_SIZE {
        return Err(ModelsError::BufferError(
            "slice too small to extract array".into(),
        ));
    }
    buffer[..ARRAY_SIZE].try_into().map_err(|err| {
        ModelsError::BufferError(format!("could not extract array from slice: {}", err))
    })
}

pub fn u8_from_slice(buffer: &[u8]) -> Result<u8, ModelsError> {
    if buffer.is_empty() {
        return Err(ModelsError::BufferError(
            "could not read u8 from empty buffer".into(),
        ));
    }
    Ok(buffer[0])
}

pub trait SerializeCompact {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError>;
}

pub trait DeserializeCompact: Sized {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError>;
}

/// Checks performed:
/// - Buffer contains a valid u8(implicit check).
impl SerializeCompact for IpAddr {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        Ok(match self {
            IpAddr::V4(ip_v4) => {
                let mut res = Vec::with_capacity(1 + 4);
                res.push(4u8);
                res.extend(&ip_v4.octets());
                res
            }
            IpAddr::V6(ip_v6) => {
                let mut res = Vec::with_capacity(1 + 16);
                res.push(6u8);
                res.extend(&ip_v6.octets());
                res
            }
        })
    }
}

/// Checks performed:
/// - Buffer contains a valid u8.
impl DeserializeCompact for IpAddr {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        match u8_from_slice(buffer)? {
            4u8 => Ok((IpAddr::V4(array_from_slice(&buffer[1..])?.into()), 1 + 4)),
            6u8 => Ok((IpAddr::V6(array_from_slice(&buffer[1..])?.into()), 1 + 16)),
            _ => Err(ModelsError::DeserializeError(
                "unsupported IpAddr variant".into(),
            )),
        }
    }
}

impl SerializeCompact for MassaTime {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        Ok(self.to_millis().to_varint_bytes())
    }
}

/// Checks performed:
/// - Buffer contains a valid u64.
impl DeserializeCompact for MassaTime {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let (res_u64, delta) = u64::from_varint_bytes(buffer)?;
        Ok((res_u64.into(), delta))
    }
}

impl SerializeCompact for Amount {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        Ok(self.to_raw().to_varint_bytes())
    }
}

/// Checks performed:
/// - Buffer contains a valid u8.
impl DeserializeCompact for Amount {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let (res_u64, delta) = u64::from_varint_bytes(buffer)?;
        Ok((Amount::from_raw(res_u64), delta))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_varint() {
        let x32 = 70_000u32;
        let x64 = 10_000_000_000u64;

        // serialize
        let mut res: Vec<u8> = Vec::new();
        res.extend(x32.to_varint_bytes());
        assert_eq!(res.len(), 3);
        res.extend(x64.to_varint_bytes());
        assert_eq!(res.len(), 3 + 5);

        // deserialize
        let buf = res.as_slice();
        let mut cursor = 0;
        let (out_x32, delta) = u32::from_varint_bytes_bounded(&buf[cursor..], 80_000).unwrap();
        assert_eq!(out_x32, x32);
        cursor += delta;
        let (out_x64, delta) =
            u64::from_varint_bytes_bounded(&buf[cursor..], 20_000_000_000).unwrap();
        assert_eq!(out_x64, x64);
        cursor += delta;
        assert_eq!(cursor, buf.len());

        // deserialize fail bounds
        assert!(u32::from_varint_bytes_bounded(&buf[0..], 69_999).is_err());
        assert!(u64::from_varint_bytes_bounded(&buf[3..], 9_999_999_999).is_err());
    }

    #[test]
    #[serial]
    fn test_be_min() {
        let x32 = 70_000u32;
        let x64 = 10_000_000_000u64;

        // serialize
        let mut res: Vec<u8> = Vec::new();
        res.extend(x32.to_be_bytes_min(70_001).unwrap());
        assert_eq!(res.len(), 3);
        res.extend(x64.to_be_bytes_min(10_000_000_001).unwrap());
        assert_eq!(res.len(), 3 + 5);

        // serialize fail bounds
        assert!(x32.to_be_bytes_min(69_999).is_err());
        assert!(x64.to_be_bytes_min(9_999_999_999).is_err());

        // deserialize
        let buf = res.as_slice();
        let mut cursor = 0;
        let (out_x32, delta) = u32::from_be_bytes_min(&buf[cursor..], 70_001).unwrap();
        assert_eq!(out_x32, x32);
        cursor += delta;
        let (out_x64, delta) = u64::from_be_bytes_min(&buf[cursor..], 10_000_000_001).unwrap();
        assert_eq!(out_x64, x64);
        cursor += delta;
        assert_eq!(cursor, buf.len());
    }

    #[test]
    #[serial]
    fn test_array_from_slice_with_zero_u64() {
        let zero: u64 = 0;
        let res = array_from_slice(&zero.to_be_bytes()).unwrap();
        assert_eq!(zero, u64::from_be_bytes(res));
    }
}
