// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::ModelsError;
use crate::Amount;
use integer_encoding::VarInt;
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::bytes::complete::take;
use nom::multi::length_data;
use nom::sequence::preceded;
use nom::{
    error::{context, ContextError, ParseError},
    IResult,
};
use nom::{Parser, ToUsize};
use std::convert::TryInto;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::ops::Bound;

/// varint serialization
pub trait SerializeVarInt {
    /// Serialize as varint bytes
    fn to_varint_bytes(self) -> Vec<u8>;
}

impl SerializeVarInt for u16 {
    fn to_varint_bytes(self) -> Vec<u8> {
        self.encode_var_vec()
    }
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

/// var int deserialization
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

impl DeserializeVarInt for u16 {
    fn from_varint_bytes(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        u16::decode_var(buffer)
            .ok_or_else(|| ModelsError::DeserializeError("could not deserialize varint".into()))
    }

    fn from_varint_bytes_bounded(
        buffer: &[u8],
        max_value: Self,
    ) -> Result<(Self, usize), ModelsError> {
        let (res, res_size) = Self::from_varint_bytes(buffer)?;
        if res > max_value {
            return Err(ModelsError::DeserializeError(
                "deserialized varint u16 out of bounds".into(),
            ));
        }
        Ok((res, res_size))
    }
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

/// Serialize min big endian integer
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

/// Deserialize min big endian
pub trait DeserializeMinBEInt: Sized {
    /// min big endian integer base size
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

/// array from slice
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

/// `u8` from slice
pub fn u8_from_slice(buffer: &[u8]) -> Result<u8, ModelsError> {
    if buffer.is_empty() {
        return Err(ModelsError::BufferError(
            "could not read u8 from empty buffer".into(),
        ));
    }
    Ok(buffer[0])
}

/// custom serialization trait
pub trait SerializeCompact {
    /// serialization
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError>;
}

/// custom deserialization trait
pub trait DeserializeCompact: Sized {
    /// deserialization
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError>;
}

/// Serializer for `IpAddr`
#[derive(Default)]
pub struct IpAddrSerializer;

impl IpAddrSerializer {
    /// Creates a `IpAddrSerializer`
    pub const fn new() -> Self {
        Self
    }
}

impl Serializer<IpAddr> for IpAddrSerializer {
    /// ```
    /// use massa_models::{Address, Amount, Slot, IpAddrSerializer};
    /// use massa_serialization::Serializer;
    /// use std::str::FromStr;
    /// use std::net::{IpAddr, Ipv4Addr};
    ///
    /// let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    /// let ip_serializer = IpAddrSerializer::new();
    /// let mut buffer = Vec::new();
    /// ip_serializer.serialize(&ip, &mut buffer).unwrap();
    /// ```
    fn serialize(&self, value: &IpAddr, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        match value {
            IpAddr::V4(ip_v4) => {
                buffer.push(4u8);
                buffer.extend(&ip_v4.octets());
            }
            IpAddr::V6(ip_v6) => {
                buffer.push(6u8);
                buffer.extend(&ip_v6.octets());
            }
        };
        Ok(())
    }
}

/// Deserializer for `IpAddr`
#[derive(Default)]
pub struct IpAddrDeserializer;

impl IpAddrDeserializer {
    /// Creates a `IpAddrDeserializer`
    pub const fn new() -> Self {
        Self
    }
}

impl Deserializer<IpAddr> for IpAddrDeserializer {
    /// ```
    /// use massa_models::{Address, Amount, Slot, IpAddrSerializer, IpAddrDeserializer};
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use std::str::FromStr;
    /// use std::net::{IpAddr, Ipv4Addr};
    ///
    /// let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    /// let ip_serializer = IpAddrSerializer::new();
    /// let ip_deserializer = IpAddrDeserializer::new();
    /// let mut serialized = Vec::new();
    /// ip_serializer.serialize(&ip, &mut serialized).unwrap();
    /// let (rest, ip_deser) = ip_deserializer.deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(ip, ip_deser);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], IpAddr, E> {
        nom::branch::alt((
            preceded(
                |input| nom::bytes::complete::tag([4u8])(input),
                |input: &'a [u8]| {
                    let (rest, addr) = take(4usize)(input)?;
                    let addr: [u8; 4] = addr.try_into().unwrap();
                    Ok((rest, IpAddr::V4(Ipv4Addr::from(addr))))
                },
            ),
            preceded(
                |input| nom::bytes::complete::tag([6u8])(input),
                |input: &'a [u8]| {
                    let (rest, addr) = take(16usize)(input)?;
                    // Safe because take would fail just above if less then 16
                    let addr: [u8; 16] = addr.try_into().unwrap();
                    Ok((rest, IpAddr::V6(Ipv6Addr::from(addr))))
                },
            ),
        ))(buffer)
    }
}

impl SerializeCompact for Amount {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        Ok(self.to_raw().to_varint_bytes())
    }
}

/// Checks performed:
/// - Buffer contains a valid `u8`.
impl DeserializeCompact for Amount {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let (res_u64, delta) = u64::from_varint_bytes(buffer)?;
        Ok((Amount::from_raw(res_u64), delta))
    }
}

/// Basic `Vec<u8>` serializer
pub struct VecU8Serializer {
    len_serializer: U64VarIntSerializer,
}

impl VecU8Serializer {
    /// Creates a new `VecU8Serializer`
    pub fn new() -> Self {
        Self {
            len_serializer: U64VarIntSerializer::new(),
        }
    }
}

impl Default for VecU8Serializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<Vec<u8>> for VecU8Serializer {
    /// ```
    /// use std::ops::Bound::Included;
    /// use massa_serialization::Serializer;
    /// use massa_models::VecU8Serializer;
    ///
    /// let vec = vec![1, 2, 3];
    /// let mut buffer = Vec::new();
    /// let serializer = VecU8Serializer::new();
    /// serializer.serialize(&vec, &mut buffer).unwrap();
    /// ```
    fn serialize(&self, value: &Vec<u8>, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        let len: u64 = value.len().try_into().map_err(|err| {
            SerializeError::NumberTooBig(format!("too many entries data in VecU8: {}", err))
        })?;
        self.len_serializer.serialize(&len, buffer)?;
        buffer.extend(value);
        Ok(())
    }
}

/// Basic `Vec<u8>` deserializer
pub struct VecU8Deserializer {
    varint_u64_deserializer: U64VarIntDeserializer,
}

impl VecU8Deserializer {
    /// Creates a new `VecU8Deserializer`
    pub const fn new(min_length: Bound<u64>, max_length: Bound<u64>) -> Self {
        Self {
            varint_u64_deserializer: U64VarIntDeserializer::new(min_length, max_length),
        }
    }
}

impl Deserializer<Vec<u8>> for VecU8Deserializer {
    /// ```
    /// use std::ops::Bound::Included;
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use massa_models::{VecU8Serializer, VecU8Deserializer};
    ///
    /// let vec = vec![1, 2, 3];
    /// let mut serialized = Vec::new();
    /// let serializer = VecU8Serializer::new();
    /// let deserializer = VecU8Deserializer::new(Included(0), Included(1000000));
    /// serializer.serialize(&vec, &mut serialized).unwrap();
    /// let (rest, vec_deser) = deserializer.deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(vec, vec_deser);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Vec<u8>, E> {
        context("Failed Vec<u8> deserialization", |input| {
            length_data(|input| self.varint_u64_deserializer.deserialize(input))(input)
        })
        .map(|res| res.to_vec())
        .parse(buffer)
    }
}

/// Serializer for `String` with generic serializer for the size of the string
pub struct StringSerializer<SL, L>
where
    SL: Serializer<L>,
    L: TryFrom<usize>,
{
    length_serializer: SL,
    marker_l: std::marker::PhantomData<L>,
}

impl<SL, L> StringSerializer<SL, L>
where
    SL: Serializer<L>,
    L: TryFrom<usize>,
{
    /// Creates a `StringSerializer`.
    ///
    /// # Arguments:
    /// - `length_serializer`: Serializer for the length of the string (should be one of `UXXVarIntSerializer`)
    pub fn new(length_serializer: SL) -> Self {
        Self {
            length_serializer,
            marker_l: std::marker::PhantomData,
        }
    }
}

impl<SL, L> Serializer<String> for StringSerializer<SL, L>
where
    SL: Serializer<L>,
    L: TryFrom<usize>,
{
    fn serialize(&self, value: &String, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.length_serializer.serialize(
            &value.len().try_into().map_err(|_| {
                SerializeError::StringTooBig("The string is too big to be serialized".to_string())
            })?,
            buffer,
        )?;
        buffer.extend(value.as_bytes());
        Ok(())
    }
}

/// Deserializer for `String` with generic deserializer for the size of the string
pub struct StringDeserializer<DL, L>
where
    DL: Deserializer<L>,
    L: TryFrom<usize> + ToUsize,
{
    length_deserializer: DL,
    marker_l: std::marker::PhantomData<L>,
}

impl<DL, L> StringDeserializer<DL, L>
where
    DL: Deserializer<L>,
    L: TryFrom<usize> + ToUsize,
{
    /// Creates a `StringDeserializer`.
    ///
    /// # Arguments:
    /// - `length_deserializer`: Serializer for the length of the string (should be one of `UXXVarIntSerializer`)
    pub const fn new(length_deserializer: DL) -> Self {
        Self {
            length_deserializer,
            marker_l: std::marker::PhantomData,
        }
    }
}

impl<DL, L> Deserializer<String> for StringDeserializer<DL, L>
where
    DL: Deserializer<L>,
    L: TryFrom<usize> + ToUsize,
{
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], String, E> {
        let (rest, res) = length_data(|input| self.length_deserializer.deserialize(input))
            .map(|data| {
                String::from_utf8(data.to_vec()).map_err(|_| {
                    nom::Err::Error(ParseError::from_error_kind(
                        data,
                        nom::error::ErrorKind::Verify,
                    ))
                })
            })
            .parse(buffer)?;
        Ok((rest, res?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use massa_serialization::DeserializeError;
    use serial_test::serial;
    use std::ops::Bound::Included;
    #[test]
    #[serial]
    fn vec_u8() {
        let vec: Vec<u8> = vec![9, 8, 7];
        let vec_u8_serializer = VecU8Serializer::new();
        let vec_u8_deserializer = VecU8Deserializer::new(Included(u64::MIN), Included(u64::MAX));
        let mut serialized = Vec::new();
        vec_u8_serializer.serialize(&vec, &mut serialized).unwrap();
        let (rest, new_vec) = vec_u8_deserializer
            .deserialize::<DeserializeError>(&serialized)
            .unwrap();
        assert!(rest.is_empty());
        assert_eq!(vec, new_vec);
    }

    #[test]
    #[serial]
    fn vec_u8_big_length() {
        let vec: Vec<u8> = vec![9, 8, 7];
        let len: u64 = 10;
        let mut serialized = Vec::new();
        U64VarIntSerializer::new()
            .serialize(&len, &mut serialized)
            .unwrap();
        serialized.extend(vec);
        let vec_u8_deserializer = VecU8Deserializer::new(Included(u64::MIN), Included(u64::MAX));
        let _ = vec_u8_deserializer
            .deserialize::<DeserializeError>(&serialized)
            .expect_err("Should fail too long size");
    }

    #[test]
    #[serial]
    fn vec_u8_min_length() {
        let vec: Vec<u8> = vec![9, 8, 7];
        let len: u64 = 1;
        let mut serialized = Vec::new();
        U64VarIntSerializer::new()
            .serialize(&len, &mut serialized)
            .unwrap();
        serialized.extend(vec);
        let vec_u8_deserializer = VecU8Deserializer::new(Included(u64::MIN), Included(u64::MAX));
        let (rest, res) = vec_u8_deserializer
            .deserialize::<DeserializeError>(&serialized)
            .unwrap();
        assert_eq!(rest, &[8, 7]);
        assert_eq!(res, &[9])
    }

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
