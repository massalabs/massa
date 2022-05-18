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

/// TODO: Doc
pub trait Deserializer<T> {
    /// TODO: Doc
    fn deserialize<'a>(&self, buffer: &'a [u8]) -> IResult<&'a [u8], T>;
}

/// TODO: Doc
pub trait Serializer<T> {
    /// TODO: Doc
    fn serialize(&self, value: &T) -> Result<Vec<u8>, SerializeError>;
}
