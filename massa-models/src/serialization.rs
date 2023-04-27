// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::ModelsError;
use crate::prehash::{PreHashSet, PreHashed};
use bitvec::prelude::BitVec;
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U32VarIntDeserializer, U32VarIntSerializer,
    U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::bytes::complete::take;
use nom::multi::{length_count, length_data};
use nom::sequence::preceded;
use nom::{branch::alt, Parser, ToUsize};
use nom::{
    error::{context, ContextError, ErrorKind, ParseError},
    IResult,
};
use std::convert::TryInto;
use std::marker::PhantomData;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::ops::Bound;
use Bound::Included;

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
            return Err(ModelsError::SerializeError("unexpected buffer END".into()));
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
            return Err(ModelsError::SerializeError("unexpected buffer END".into()));
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

/// Serializer for `IpAddr`
#[derive(Default, Clone)]
pub struct IpAddrSerializer;

impl IpAddrSerializer {
    /// Creates a `IpAddrSerializer`
    pub const fn new() -> Self {
        Self
    }
}

impl Serializer<IpAddr> for IpAddrSerializer {
    /// ```
    /// use massa_models::{address::Address, amount::Amount, slot::Slot, serialization::IpAddrSerializer};
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
                buffer.extend(ip_v4.octets());
            }
            IpAddr::V6(ip_v6) => {
                buffer.push(6u8);
                buffer.extend(ip_v6.octets());
            }
        };
        Ok(())
    }
}

/// Deserializer for `IpAddr`
#[derive(Default, Clone)]
pub struct IpAddrDeserializer;

impl IpAddrDeserializer {
    /// Creates a `IpAddrDeserializer`
    pub const fn new() -> Self {
        Self
    }
}

impl Deserializer<IpAddr> for IpAddrDeserializer {
    /// ```
    /// use massa_models::{address::Address, amount::Amount, slot::Slot, serialization::{IpAddrSerializer, IpAddrDeserializer}};
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
        context(
            "Failed IpAddr deserialization",
            alt((
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
            )),
        )(buffer)
    }
}

/// Basic `Vec<u8>` serializer
#[derive(Clone)]
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
    /// use massa_models::serialization::VecU8Serializer;
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
#[derive(Clone)]
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
    /// use massa_models::serialization::{VecU8Serializer, VecU8Deserializer};
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

/// Basic `Vec<_>` serializer
#[derive(Clone)]
pub struct VecSerializer<T, ST>
where
    ST: Serializer<T>,
{
    len_serializer: U64VarIntSerializer,
    data_serializer: ST,
    phantom_t: PhantomData<T>,
}

impl<T, ST> VecSerializer<T, ST>
where
    ST: Serializer<T>,
{
    /// Creates a new `VecSerializer`
    pub fn new(data_serializer: ST) -> Self {
        Self {
            len_serializer: U64VarIntSerializer::new(),
            data_serializer,
            phantom_t: PhantomData,
        }
    }
}

impl<T, ST> Serializer<Vec<T>> for VecSerializer<T, ST>
where
    ST: Serializer<T>,
{
    fn serialize(&self, value: &Vec<T>, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.len_serializer
            .serialize(&(value.len() as u64), buffer)?;
        for elem in value {
            self.data_serializer.serialize(elem, buffer)?;
        }
        Ok(())
    }
}

/// Basic `Vec<_>` deserializer
#[derive(Clone)]
pub struct VecDeserializer<T, ST>
where
    ST: Deserializer<T> + Clone,
{
    varint_u64_deserializer: U64VarIntDeserializer,
    data_deserializer: ST,
    phantom_t: PhantomData<T>,
}

impl<T, ST> VecDeserializer<T, ST>
where
    ST: Deserializer<T> + Clone,
{
    /// Creates a new `VecDeserializer`
    pub const fn new(
        data_deserializer: ST,
        min_length: Bound<u64>,
        max_length: Bound<u64>,
    ) -> Self {
        Self {
            varint_u64_deserializer: U64VarIntDeserializer::new(min_length, max_length),
            data_deserializer,
            phantom_t: PhantomData,
        }
    }
}

impl<T, ST> Deserializer<Vec<T>> for VecDeserializer<T, ST>
where
    ST: Deserializer<T> + Clone,
{
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Vec<T>, E> {
        context("Failed Vec<_> deserialization", |input| {
            length_count(
                context("length", |input| {
                    self.varint_u64_deserializer.deserialize(input)
                }),
                context("data", |input| self.data_deserializer.deserialize(input)),
            )(input)
        })
        .parse(buffer)
    }
}

/// Basic `PreHashSet<_>` serializer
#[derive(Clone)]
pub struct PreHashSetSerializer<T, ST>
where
    ST: Serializer<T>,
{
    len_serializer: U64VarIntSerializer,
    data_serializer: ST,
    phantom_t: PhantomData<T>,
}

impl<T, ST> PreHashSetSerializer<T, ST>
where
    ST: Serializer<T>,
{
    /// Creates a new `PreHashSetSerializer`
    pub fn new(data_serializer: ST) -> Self {
        Self {
            len_serializer: U64VarIntSerializer::new(),
            data_serializer,
            phantom_t: PhantomData,
        }
    }
}

impl<T, ST> Serializer<PreHashSet<T>> for PreHashSetSerializer<T, ST>
where
    ST: Serializer<T>,
    T: PreHashed,
{
    fn serialize(&self, value: &PreHashSet<T>, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.len_serializer
            .serialize(&(value.len() as u64), buffer)?;
        for elem in value {
            self.data_serializer.serialize(elem, buffer)?;
        }
        Ok(())
    }
}

/// Basic `PreHashSet<_>` deserializer
#[derive(Clone)]
pub struct PreHashSetDeserializer<T, ST>
where
    ST: Deserializer<T> + Clone,
{
    varint_u64_deserializer: U64VarIntDeserializer,
    data_deserializer: ST,
    phantom_t: PhantomData<T>,
}

impl<T, ST> PreHashSetDeserializer<T, ST>
where
    ST: Deserializer<T> + Clone,
{
    /// Creates a new `PreHashSetDeserializer`
    pub const fn new(
        data_deserializer: ST,
        min_length: Bound<u64>,
        max_length: Bound<u64>,
    ) -> Self {
        Self {
            varint_u64_deserializer: U64VarIntDeserializer::new(min_length, max_length),
            data_deserializer,
            phantom_t: PhantomData,
        }
    }
}

impl<T, ST> Deserializer<PreHashSet<T>> for PreHashSetDeserializer<T, ST>
where
    ST: Deserializer<T> + Clone,
    T: PreHashed + std::cmp::Eq + std::hash::Hash,
{
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], PreHashSet<T>, E> {
        context("Failed PreHashSet<_> deserialization", |input| {
            length_count(
                context("length", |input| {
                    self.varint_u64_deserializer.deserialize(input)
                }),
                context("data", |input| self.data_deserializer.deserialize(input)),
            )(input)
        })
        .map(|vec| vec.into_iter().collect())
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

/// `BitVec<u8>` Serializer
pub struct BitVecSerializer {
    u32_serializer: U32VarIntSerializer,
}

impl BitVecSerializer {
    /// Create a new `BitVec<u8>` Serializer
    pub fn new() -> BitVecSerializer {
        BitVecSerializer {
            u32_serializer: U32VarIntSerializer::new(),
        }
    }
}

impl Default for BitVecSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<BitVec<u8>> for BitVecSerializer {
    fn serialize(&self, value: &BitVec<u8>, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        let n_entries: u32 = value.len().try_into().map_err(|err| {
            SerializeError::NumberTooBig(format!(
                "too many entries when serializing a `BitVec<u8>`: {}",
                err
            ))
        })?;
        self.u32_serializer.serialize(&n_entries, buffer)?;
        buffer.extend(value.clone().into_vec());
        Ok(())
    }
}

/// `BitVec<u8>` Deserializer
pub struct BitVecDeserializer {
    u32_deserializer: U32VarIntDeserializer,
}

impl BitVecDeserializer {
    /// Create a new `BitVec<u8>` Deserializer
    pub fn new() -> BitVecDeserializer {
        BitVecDeserializer {
            u32_deserializer: U32VarIntDeserializer::new(
                Bound::Included(u32::MIN),
                Included(u32::MAX),
            ),
        }
    }
}

impl Default for BitVecDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<BitVec<u8>> for BitVecDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BitVec<u8>, E> {
        context("Failed rng_seed deserialization", |input| {
            let (rest, n_entries) = self.u32_deserializer.deserialize(input)?;
            let bits_u8_len = n_entries.div_ceil(u8::BITS) as usize;
            if rest.len() < bits_u8_len {
                return Err(nom::Err::Error(ParseError::from_error_kind(
                    input,
                    ErrorKind::Eof,
                )));
            }
            let mut rng_seed: BitVec<u8> = BitVec::try_from_vec(rest[..bits_u8_len].to_vec())
                .map_err(|_| nom::Err::Error(ParseError::from_error_kind(input, ErrorKind::Eof)))?;
            rng_seed.truncate(n_entries as usize);
            if rng_seed.len() != n_entries as usize {
                return Err(nom::Err::Error(ParseError::from_error_kind(
                    input,
                    ErrorKind::Eof,
                )));
            }
            Ok((&rest[bits_u8_len..], rng_seed))
        })
        .map(|elements| elements.into_iter().collect())
        .parse(buffer)
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
