use displaydoc::Display;
use nom::IResult;
use thiserror::Error;

#[non_exhaustive]
#[derive(Display, Error, Debug, Clone)]
pub enum SerializeError {
    /// Number {0} is too big to be serialized
    NumberTooBig(String),
    /// General error {0}
    GeneralError(String),
}

/// Trait that define the deserialize method that must be implemented for all types have serialize form in Massa.
///
/// This trait must be implemented on deserializers that will be defined for each type and can contains constraints.
/// Example:
/// ```
/// use std::ops::Bound;
/// use unsigned_varint::nom as varint_nom;
/// use nom::IResult;
/// use massa_serialization::Deserializer;
/// use std::ops::RangeBounds;
///
/// pub struct U64VarIntDeserializer {
///     range: (Bound<u64>, Bound<u64>)
/// }
///
/// impl U64VarIntDeserializer {
///     fn new(min: Bound<u64>, max: Bound<u64>) -> Self {
///         Self {
///             range: (min, max)
///         }
///     }
/// }
///
/// impl Deserializer<u64> for U64VarIntDeserializer {
///     fn deserialize<'a>(&self, buffer: &'a [u8]) -> IResult<&'a [u8], u64> {
///         let (rest, value) = varint_nom::u64(buffer)?;
///         if !self.range.contains(&value) {
///             return Err(nom::Err::Error(nom::error::Error::new(buffer, nom::error::ErrorKind::TooLarge)));
///         }
///         Ok((rest, value))
///     }
/// }
/// ```
pub trait Deserializer<T> {
    /// Deserialize a value `T` from a buffer of `u8`.
    ///
    /// ## Parameters
    /// * buffer: the buffer that contains the whole serialized data.
    ///
    /// ## Returns
    /// A nom result with the rest of the serialized data and the decoded value.
    fn deserialize<'a>(&self, buffer: &'a [u8]) -> IResult<&'a [u8], T>;
}

/// This trait must be implemented to serializes all data in Massa.
///
/// Example:
/// ```
/// use std::ops::Bound;
/// use unsigned_varint::nom as varint_nom;
/// use nom::IResult;
/// use massa_serialization::Serializer;
/// use std::ops::RangeBounds;
/// use unsigned_varint::encode::u64_buffer;
/// use unsigned_varint::encode::u64;
/// use massa_serialization::SerializeError;
///
/// pub struct U64VarIntSerializer {
///     range: (Bound<u64>, Bound<u64>)
/// }
///
/// impl U64VarIntSerializer {
///     fn new(min: Bound<u64>, max: Bound<u64>) -> Self {
///         Self {
///             range: (min, max)
///         }
///     }
/// }
///
/// impl Serializer<u64> for U64VarIntSerializer {
///     fn serialize(&self, value: &u64) -> Result<Vec<u8>, SerializeError> {
///         if !self.range.contains(value) {
///             return Err(SerializeError::NumberTooBig(format!("Value {:#?} is not in range {:#?}", value, self.range)));
///         }
///         Ok(u64(*value, &mut u64_buffer()).to_vec())
///     }
/// }
/// ```
pub trait Serializer<T> {
    /// Serialize a value `T` into a buffer of `u8`.
    ///
    /// ## Parameters
    /// * value: the value to be serialized.
    ///
    /// ## Returns
    /// A Result with the serialized data.
    fn serialize(&self, value: &T) -> Result<Vec<u8>, SerializeError>;
}
