use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ParseError},
    IResult, Parser,
};
use std::{marker::PhantomData, ops::Bound::Included};

/// Streaming step cursor
#[derive(PartialEq, Eq, Copy, Clone, Debug)]
pub enum StreamingStep<T> {
    /// Started step, only when launching the streaming
    Started,
    /// Ongoing step, as long as you are streaming
    Ongoing(T),
    /// Finished step, after all the information has been streamed
    ///
    /// Also can keep an indicator of the last content streamed
    Finished,
}

impl<T> StreamingStep<T> {
    /// Indicates if the current step if finished or not without caring about the values
    pub fn finished(&self) -> bool {
        matches!(self, StreamingStep::Finished)
    }
}

/// `StreamingStep` serializer
pub struct StreamingStepSerializer<T, ST>
where
    ST: Serializer<T>,
{
    u64_serializer: U64VarIntSerializer,
    data_serializer: ST,
    phantom_t: PhantomData<T>,
}

impl<T, ST> StreamingStepSerializer<T, ST>
where
    ST: Serializer<T>,
{
    /// Creates a new `StreamingStep` serializer
    pub fn new(data_serializer: ST) -> Self {
        Self {
            u64_serializer: U64VarIntSerializer::new(),
            data_serializer,
            phantom_t: PhantomData,
        }
    }
}

impl<T, ST> Serializer<StreamingStep<T>> for StreamingStepSerializer<T, ST>
where
    ST: Serializer<T>,
{
    fn serialize(
        &self,
        value: &StreamingStep<T>,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        match value {
            StreamingStep::Started => self.u64_serializer.serialize(&0u64, buffer)?,
            StreamingStep::Ongoing(cursor_data) => {
                self.u64_serializer.serialize(&1u64, buffer)?;
                self.data_serializer.serialize(cursor_data, buffer)?;
            }
            StreamingStep::Finished => self.u64_serializer.serialize(&2u64, buffer)?,
        };
        Ok(())
    }
}

/// `StreamingStep` deserializer
pub struct StreamingStepDeserializer<T, ST>
where
    ST: Deserializer<T>,
{
    u64_deserializer: U64VarIntDeserializer,
    data_deserializer: ST,
    phantom_t: PhantomData<T>,
}

impl<T, ST> StreamingStepDeserializer<T, ST>
where
    ST: Deserializer<T>,
{
    /// Creates a new `StreamingStep` deserializer
    pub fn new(data_deserializer: ST) -> Self {
        Self {
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
            data_deserializer,
            phantom_t: PhantomData,
        }
    }
}

impl<T, ST> Deserializer<StreamingStep<T>> for StreamingStepDeserializer<T, ST>
where
    ST: Deserializer<T>,
{
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], StreamingStep<T>, E> {
        context("StreamingStep", |input| {
            let (rest, ident) = context("identifier", |input| {
                self.u64_deserializer.deserialize(input)
            })
            .parse(input)?;
            match ident {
                0u64 => Ok((rest, StreamingStep::Started)),
                1u64 => context("data", |input| self.data_deserializer.deserialize(input))
                    .map(StreamingStep::Ongoing)
                    .parse(rest),
                2u64 => Ok((rest, StreamingStep::Finished)),
                _ => Err(nom::Err::Error(ParseError::from_error_kind(
                    buffer,
                    nom::error::ErrorKind::Digit,
                ))),
            }
        })
        .parse(buffer)
    }
}
