use std::{
    collections::VecDeque,
    fmt::{Debug, Display},
};

use displaydoc::Display;
use nom::{
    branch::alt,
    bytes::complete::tag,
    combinator::value,
    error::{ContextError, ParseError},
    sequence::preceded,
    sequence::tuple,
    IResult, Parser,
};
use num::rational::Ratio;
use num::Integer;
use thiserror::Error;

#[non_exhaustive]
#[derive(Display, Error, Debug, Clone)]
pub enum SerializeError {
    /// Number {0} is too big to be serialized
    NumberTooBig(String),
    /// General error {0}
    GeneralError(String),
    /// String too big {0},
    StringTooBig(String),
}

#[derive(Clone, Error)]
pub struct DeserializeError<'a> {
    errors: VecDeque<(&'a [u8], String)>,
}

impl<'a> ContextError<&'a [u8]> for DeserializeError<'a> {
    fn add_context(input: &'a [u8], ctx: &'static str, mut other: Self) -> Self {
        other.errors.push_front((input, ctx.to_string()));
        other
    }
}

impl<'a> ParseError<&'a [u8]> for DeserializeError<'a> {
    fn append(input: &'a [u8], kind: nom::error::ErrorKind, mut other: Self) -> Self {
        other
            .errors
            .push_front((input, kind.description().to_string()));
        other
    }
    fn from_error_kind(input: &'a [u8], kind: nom::error::ErrorKind) -> Self {
        let mut errors = VecDeque::new();
        errors.push_front((input, kind.description().to_string()));
        Self { errors }
    }
    fn from_char(input: &'a [u8], _: char) -> Self {
        Self::from_error_kind(input, nom::error::ErrorKind::Char)
    }
    fn or(self, other: Self) -> Self {
        other
    }
}

impl<'a> Display for DeserializeError<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for error in &self.errors {
            write!(f, "{} / ", error.1)?;
        }
        Ok(())
    }
}

impl<'a> Debug for DeserializeError<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut last_input = None;
        for error in &self.errors {
            write!(f, "{} / ", error.1)?;
            last_input = Some(error.0);
        }
        if let Some(last_input) = last_input {
            writeln!(f, "Input: {:?}", last_input)?;
        }
        Ok(())
    }
}

/// Trait that define the deserialize method that must be implemented for all types have serialize form in Massa.
///
/// This trait must be implemented on deserializers that will be defined for each type and can contains constraints.
/// Example:
/// ```
/// use std::ops::Bound;
/// use unsigned_varint::nom as varint_nom;
/// use nom::{IResult, error::{context, ContextError, ParseError}};
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
///     fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(&self, buffer: &'a [u8]) -> IResult<&'a [u8], u64, E> {
///         context(concat!("Failed u64 deserialization"), |input: &'a [u8]| {
///             let (rest, value) = varint_nom::u64(input).map_err(|_| nom::Err::Error(ParseError::from_error_kind(input, nom::error::ErrorKind::Fail)))?;
///             if !self.range.contains(&value) {
///                 return Err(nom::Err::Error(ParseError::from_error_kind(input, nom::error::ErrorKind::Fail)));
///             }
///             Ok((rest, value))
///         })(buffer)
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
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], T, E>;
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
///     fn serialize(&self, value: &u64, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
///         if !self.range.contains(value) {
///             return Err(SerializeError::NumberTooBig(format!("Value {:#?} is not in range {:#?}", value, self.range)));
///         }
///         buffer.extend_from_slice(u64(*value, &mut u64_buffer()));
///         Ok(())
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
    fn serialize(&self, value: &T, buffer: &mut Vec<u8>) -> Result<(), SerializeError>;
}

macro_rules! gen_varint {
    ($($type:ident, $s:ident, $bs:ident, $ds:ident, $d:expr);*) => {
        use std::ops::{Bound, RangeBounds};
        use nom::error::context;
        use unsigned_varint::nom as unsigned_nom;
        $(
            use unsigned_varint::encode::{$type, $bs};
            #[doc = " Serializer for "]
            #[doc = $d]
            #[doc = " in a varint form."]
            #[derive(Clone)]
            pub struct $s;

            impl $s {
                #[doc = "Create a basic serializer for "]
                #[doc = $d]
                #[doc = " in a varint form."]
                #[allow(dead_code)]
                pub const fn new() -> Self {
                    Self
                }
            }

            impl Default for $s {
                fn default() -> $s {
                    $s::new()
                }
            }

            impl Serializer<$type> for $s {
                fn serialize(&self, value: &$type, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
                    buffer.extend_from_slice($type(*value, &mut $bs()));
                    Ok(())
                }
            }

            #[doc = " Deserializer for "]
            #[doc = $d]
            #[doc = " in a varint form."]
            #[derive(Clone)]
            pub struct $ds {
                range: (Bound<$type>, Bound<$type>)
            }

            impl $ds {
                #[doc = "Create a basic deserializer for "]
                #[doc = $d]
                #[doc = " in a varint form."]
                #[allow(dead_code)]
                pub const fn new(min: Bound<$type>, max: Bound<$type>) -> Self {
                    Self {
                        range: (min, max)
                    }
                }
            }

            impl Deserializer<$type> for $ds {
                fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(&self, buffer: &'a [u8]) -> IResult<&'a [u8], $type, E> {
                    context(concat!("Failed ", stringify!($type), " deserialization"), |input: &'a [u8]| {
                        let (rest, value) = unsigned_nom::$type(input).map_err(|_| nom::Err::Error(ParseError::from_error_kind(input, nom::error::ErrorKind::Fail)))?;
                        if !self.range.contains(&value) {
                            return Err(nom::Err::Error(ParseError::from_error_kind(input, nom::error::ErrorKind::Fail)));
                        }
                        Ok((rest, value))
                    })(buffer)
                }
            }
        )*
    };
}

gen_varint! {
u16, U16VarIntSerializer, u16_buffer, U16VarIntDeserializer, "`u16`";
u32, U32VarIntSerializer, u32_buffer, U32VarIntDeserializer, "`u32`";
u64, U64VarIntSerializer, u64_buffer, U64VarIntDeserializer, "`u64`";
u128, U128VarIntSerializer, u128_buffer, U128VarIntDeserializer, "`u128`"
}

#[derive(Clone)]
pub struct OptionSerializer<T, ST>
where
    ST: Serializer<T>,
{
    data_serializer: ST,
    phantom_t: std::marker::PhantomData<T>,
}

impl<T, ST> OptionSerializer<T, ST>
where
    ST: Serializer<T>,
{
    pub fn new(data_serializer: ST) -> Self {
        OptionSerializer {
            data_serializer,
            phantom_t: std::marker::PhantomData,
        }
    }
}

impl<T, ST> Serializer<Option<T>> for OptionSerializer<T, ST>
where
    ST: Serializer<T>,
{
    fn serialize(&self, opt_value: &Option<T>, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        if let Some(value) = opt_value {
            buffer.push(b'1');
            self.data_serializer.serialize(value, buffer)?;
        } else {
            buffer.push(b'0');
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct OptionDeserializer<T, DT>
where
    T: Clone,
    DT: Deserializer<T>,
{
    data_deserializer: DT,
    phantom_t: std::marker::PhantomData<T>,
}

impl<T, DT> OptionDeserializer<T, DT>
where
    T: Clone,
    DT: Deserializer<T>,
{
    pub const fn new(data_deserializer: DT) -> Self {
        OptionDeserializer {
            data_deserializer,
            phantom_t: std::marker::PhantomData,
        }
    }
}

impl<T, DT> Deserializer<Option<T>> for OptionDeserializer<T, DT>
where
    T: Clone,
    DT: Deserializer<T>,
{
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Option<T>, E> {
        context(
            "Option<_> deserializer failed",
            alt((
                context("None", value(None, tag(b"0"))),
                context(
                    "Some(_)",
                    preceded(tag(b"1"), |input| {
                        self.data_deserializer
                            .deserialize(input)
                            .map(|(rest, data)| (rest, Some(data)))
                    }),
                ),
            )),
        )
        .parse(buffer)
    }
}

/// Serializer for bool
#[derive(Clone, Debug, Default)]
pub struct BoolSerializer {}

impl BoolSerializer {
    /// ctor
    pub fn new() -> Self {
        Self {}
    }
}

impl Serializer<bool> for BoolSerializer {
    fn serialize(&self, value: &bool, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        buffer.push(*value as u8);
        Ok(())
    }
}

/// Deserializer for bool
#[derive(Clone, Debug, Default)]
pub struct BoolDeserializer {}

impl BoolDeserializer {
    /// ctor
    pub fn new() -> Self {
        Self {}
    }
}

impl Deserializer<bool> for BoolDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], bool, E> {
        context("Failed bool deserialization", |input: &'a [u8]| {
            let Some((first, rest)) = input.split_first() else {
                return Err(nom::Err::Error(ParseError::from_error_kind(
                    input,
                    nom::error::ErrorKind::Fail,
                )));
            };
            Ok((rest, {
                match first {
                    1 => Ok(true),
                    0 => Ok(false),
                    _ => Err(nom::Err::Error(ParseError::from_error_kind(
                        input,
                        nom::error::ErrorKind::Fail,
                    ))),
                }
            }?))
        })(buffer)
    }
}

/// Serializer for Ratio
#[derive(Clone, Debug, Default)]
pub struct RatioSerializer<T, ST>
where
    T: Integer + Clone,
    ST: Serializer<T>,
{
    data_serializer: ST,
    phantom_data: std::marker::PhantomData<T>,
}

impl<T, ST> RatioSerializer<T, ST>
where
    T: Integer + Clone,
    ST: Serializer<T>,
{
    pub fn new(data_serializer: ST) -> Self {
        Self {
            data_serializer,
            phantom_data: std::marker::PhantomData,
        }
    }
}

impl<T, ST> Serializer<Ratio<T>> for RatioSerializer<T, ST>
where
    T: Integer + Clone,
    ST: Serializer<T>,
{
    fn serialize(&self, value: &Ratio<T>, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.data_serializer.serialize(value.numer(), buffer)?;
        self.data_serializer.serialize(value.denom(), buffer)?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct RatioDeserializer<T, DT>
where
    T: Integer + Clone,
    DT: Deserializer<T>,
{
    data_deserializer: DT,
    phantom_data: std::marker::PhantomData<T>,
}

impl<T, DT> RatioDeserializer<T, DT>
where
    T: Integer + Clone,
    DT: Deserializer<T>,
{
    pub fn new(data_deserializer: DT) -> Self {
        Self {
            data_deserializer,
            phantom_data: std::marker::PhantomData,
        }
    }
}

impl<T, DT> Deserializer<Ratio<T>> for RatioDeserializer<T, DT>
where
    T: Integer + Clone,
    DT: Deserializer<T>,
{
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Ratio<T>, E> {
        context(
            "Ratio<_> deserializer failed",
            tuple((
                context("numer deser failed", |input| {
                    self.data_deserializer.deserialize(input)
                }),
                context("denom deser failed", |input| {
                    self.data_deserializer.deserialize(input)
                }),
            )),
        )
        .map(|(numer, denom)| Ratio::new(numer, denom))
        .parse(buffer)
    }
}

#[cfg(test)]
mod tests {
    use crate::{DeserializeError, Deserializer, Serializer};
    use num::rational::Ratio;
    use paste::paste;

    // This macro creates a suite of tests for all types of numbers declared as parameters. Ths list of the
    // tests for each type :
    // - Test with a normal case that everything works
    // - Test with a normal case but a more bigger number that everything works
    // - Test with a number that is out of the range of the deserializer
    // - Test to give an empty buffer to the deserializer
    macro_rules! gen_test_varint {
        ($($type:ident, $bs:ident, $ds:ident);*) => {
            $(
                paste! {
                    #[test]
                    fn [<test_ $type _serializer_deserializer_works>]() {
                        let [< $type _serializer >] = super::$bs::new();
                        let number = [<3 $type >];
                        let mut buffer = Vec::new();
                        [< $type _serializer >].serialize(&number, &mut buffer).expect(concat!("Failed to serialize ", stringify!($type), " 3"));
                        assert_eq!(buffer, vec![3]);
                        let [< $type _deserializer >] = super::$ds::new(std::ops::Bound::Included([<0 $type >]), std::ops::Bound::Included(number));
                        let result = [< $type _deserializer >].deserialize::<DeserializeError>(&buffer);
                        assert!(result.is_ok());
                        let (rest, value) = result.unwrap();
                        assert!(rest.is_empty());
                        assert_eq!(value, number);
                    }

                    #[test]
                    fn [<test $type _serializer_deserializer_works_big_number>]() {
                        let [< $type _serializer >] = super::$bs::new();
                        let number = [<60_500 $type>];
                        let mut buffer = Vec::new();
                        [< $type _serializer >].serialize(&number, &mut buffer).expect(concat!("Failed to serialize ", stringify!($type), " 10_000_000"));
                        assert_eq!(buffer, vec![212, 216, 3]);
                        let [< $type _deserializer >] = super::$ds::new(std::ops::Bound::Included([<0 $type >]), std::ops::Bound::Included(number));
                        let result = [< $type _deserializer >].deserialize::<DeserializeError>(&buffer);
                        assert!(result.is_ok());
                        let (rest, value) = result.unwrap();
                        assert!(rest.is_empty());
                        assert_eq!(value, number);
                    }

                    #[test]
                    fn [<test_ $type _serializer_deserializer_bad_limits>]() {
                        let [< $type _serializer >] = super::$bs::new();
                        let number = [<3 $type >];
                        let mut buffer = Vec::new();
                        [< $type _serializer >].serialize(&number, &mut buffer).expect(concat!("Failed to serialize ", stringify!($type), " 3"));
                        assert_eq!(buffer, vec![3]);
                        let [< $type _deserializer >] = super::$ds::new(std::ops::Bound::Included([<0 $type >]), std::ops::Bound::Excluded(number));
                        let result = [< $type _deserializer >].deserialize::<DeserializeError>(&buffer);
                        assert!(result.is_err());
                        let err = result.unwrap_err();
                        assert_eq!(format!("{}", err), concat!("Parsing Error: Failed ", stringify!($type), " deserialization / Fail / Input: [3]\n"));
                    }

                    #[test]
                    fn [<test_ $type _serializer_deserializer_empty_vec>]() {
                        let buffer = vec![];
                        let [< $type _deserializer >] = super::$ds::new(std::ops::Bound::Included([<0 $type >]), std::ops::Bound::Included($type::MAX));
                        let result = [< $type _deserializer >].deserialize::<DeserializeError>(&buffer);
                        assert!(result.is_err());
                        let err = result.unwrap_err();
                        assert_eq!(format!("{}", err), concat!("Parsing Error: Failed ", stringify!($type), " deserialization / Fail / Input: []\n"));
                    }
                }
            )*
        };
    }

    gen_test_varint!(
        u16, U16VarIntSerializer, U16VarIntDeserializer;
        u32, U32VarIntSerializer, U32VarIntDeserializer;
        u64, U64VarIntSerializer, U64VarIntDeserializer
    );

    #[test]
    fn test_u64_empty_vec() {
        let buffer = vec![];
        let u64_deserializer = super::U64VarIntDeserializer::new(
            std::ops::Bound::Included(0),
            std::ops::Bound::Included(3),
        );
        let result = u64_deserializer.deserialize::<DeserializeError>(&buffer);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(
            format!("{}", err),
            "Parsing Error: Failed u64 deserialization / Fail / Input: []\n"
        );
    }

    #[test]
    fn test_option_serializer_value_works() {
        let option_serializer = super::OptionSerializer::new(super::U64VarIntSerializer::new());
        let mut buffer = Vec::new();
        option_serializer
            .serialize(&Some(3u64), &mut buffer)
            .expect("Failed to serialize Some(3)");
        assert_eq!(buffer, vec![b'1', 3]);
        let option_deserializer =
            super::OptionDeserializer::new(super::U64VarIntDeserializer::new(
                std::ops::Bound::Included(0),
                std::ops::Bound::Included(3),
            ));
        let result = option_deserializer.deserialize::<DeserializeError>(&buffer);
        assert!(result.is_ok());
        let (rest, value) = result.unwrap();
        assert!(rest.is_empty());
        assert_eq!(value, Some(3u64));
    }

    #[test]
    fn test_option_serializer_none_works() {
        let option_serializer = super::OptionSerializer::new(super::U64VarIntSerializer::new());
        let mut buffer = Vec::new();
        option_serializer
            .serialize(&None, &mut buffer)
            .expect("Failed to serialize None");
        assert_eq!(buffer, vec![b'0']);
        let option_deserializer =
            super::OptionDeserializer::new(super::U64VarIntDeserializer::new(
                std::ops::Bound::Included(0),
                std::ops::Bound::Included(3),
            ));
        let result = option_deserializer.deserialize::<DeserializeError>(&buffer);
        assert!(result.is_ok());
        let (rest, value) = result.unwrap();
        assert!(rest.is_empty());
        assert_eq!(value, None);
    }

    #[test]
    fn test_option_bad_serialized_vec() {
        let buffer = vec![2];
        let option_deserializer =
            super::OptionDeserializer::new(super::U64VarIntDeserializer::new(
                std::ops::Bound::Included(0),
                std::ops::Bound::Included(3),
            ));
        let result = option_deserializer.deserialize::<DeserializeError>(&buffer);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(format!("{}", err), "Parsing Error: Option<_> deserializer failed / Alternative / Some(_) / Tag / Input: [2]\n");
    }

    #[test]
    fn test_option_empty_vec() {
        let buffer = vec![];
        let option_deserializer =
            super::OptionDeserializer::new(super::U64VarIntDeserializer::new(
                std::ops::Bound::Included(0),
                std::ops::Bound::Included(3),
            ));
        let result = option_deserializer.deserialize::<DeserializeError>(&buffer);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(format!("{}", err), "Parsing Error: Option<_> deserializer failed / Alternative / Some(_) / Tag / Input: []\n");
    }

    #[test]
    fn test_bool_serializer_deserializer_works() {
        let bool_serializer = super::BoolSerializer::new();
        let mut buffer = Vec::new();
        bool_serializer
            .serialize(&true, &mut buffer)
            .expect("Failed to serialize true");
        assert_eq!(buffer, vec![1]);
        let bool_deserializer = super::BoolDeserializer::new();
        let result = bool_deserializer.deserialize::<DeserializeError>(&buffer);
        assert!(result.is_ok());
        let (rest, value) = result.unwrap();
        assert!(rest.is_empty());
        assert!(value);
    }

    #[test]
    fn test_bool_bad_serialized_vec() {
        let buffer = vec![2];
        let bool_deserializer = super::BoolDeserializer::new();
        let result = bool_deserializer.deserialize::<DeserializeError>(&buffer);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(
            format!("{}", err),
            "Parsing Error: Failed bool deserialization / Fail / Input: [2]\n"
        );
    }

    #[test]
    fn test_bool_empty_vec() {
        let buffer = vec![];
        let bool_deserializer = super::BoolDeserializer::new();
        let result = bool_deserializer.deserialize::<DeserializeError>(&buffer);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(
            format!("{}", err),
            "Parsing Error: Failed bool deserialization / Fail / Input: []\n"
        );
    }

    #[test]
    fn test_ratio_serializer_deserializer_works() {
        let ratio_serializer = super::RatioSerializer::new(super::U64VarIntSerializer::new());
        let mut buffer = Vec::new();
        ratio_serializer
            .serialize(&Ratio::new(3u64, 4u64), &mut buffer)
            .expect("Failed to serialize Ratio(3, 4)");
        assert_eq!(buffer, vec![3, 4]);
        let ratio_deserializer = super::RatioDeserializer::new(super::U64VarIntDeserializer::new(
            std::ops::Bound::Included(0),
            std::ops::Bound::Included(4),
        ));
        let result = ratio_deserializer.deserialize::<DeserializeError>(&buffer);
        assert!(result.is_ok());
        let (rest, value) = result.unwrap();
        assert!(rest.is_empty());
        assert_eq!(value, Ratio::new(3u64, 4u64));
    }

    #[test]
    fn test_ratio_serializer_deserializer_bad_limits() {
        let ratio_serializer = super::RatioSerializer::new(super::U64VarIntSerializer::new());
        let mut buffer = Vec::new();
        ratio_serializer
            .serialize(&Ratio::new(3u64, 4u64), &mut buffer)
            .expect("Failed to serialize Ratio(3, 4)");
        assert_eq!(buffer, vec![3, 4]);
        let ratio_deserializer = super::RatioDeserializer::new(super::U64VarIntDeserializer::new(
            std::ops::Bound::Included(0),
            std::ops::Bound::Included(3),
        ));
        let result = ratio_deserializer.deserialize::<DeserializeError>(&buffer);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(format!("{}", err), "Parsing Error: Ratio<_> deserializer failed / denom deser failed / Failed u64 deserialization / Fail / Input: [4]\n");
    }
}
