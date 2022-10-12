use massa_serialization::{
    Deserializer, OptionDeserializer, OptionSerializer, SerializeError, Serializer,
    U64VarIntDeserializer, U64VarIntSerializer,
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
    Finished(Option<T>),
}

impl<T> StreamingStep<T> {
    /// Indicates if the current step if finished or not without caring about the values
    pub fn finished(&self) -> bool {
        match self {
            StreamingStep::Finished(_) => true,
            _ => false,
        }
    }
}

/// `StreamingStep` serializer
pub struct StreamingStepSerializer<T, ST>
where
    ST: Serializer<T>,
{
    u64_serializer: U64VarIntSerializer,
    data_serializer: ST,
    option_serializer: OptionSerializer<T, ST>,
    phantom_t: PhantomData<T>,
}

impl<T, ST> StreamingStepSerializer<T, ST>
where
    ST: Serializer<T> + Clone,
{
    /// Creates a new `StreamingStep` serializer
    pub fn new(data_serializer: ST) -> Self {
        Self {
            u64_serializer: U64VarIntSerializer::new(),
            option_serializer: OptionSerializer::new(data_serializer.clone()),
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
            StreamingStep::Finished(last_data) => {
                self.u64_serializer.serialize(&2u64, buffer)?;
                self.option_serializer.serialize(last_data, buffer)?;
            }
        };
        Ok(())
    }
}

/// `StreamingStep` deserializer
pub struct StreamingStepDeserializer<T, ST>
where
    T: Clone,
    ST: Deserializer<T>,
{
    u64_deserializer: U64VarIntDeserializer,
    data_deserializer: ST,
    option_deserializer: OptionDeserializer<T, ST>,
    phantom_t: PhantomData<T>,
}

impl<T, ST> StreamingStepDeserializer<T, ST>
where
    T: Clone,
    ST: Deserializer<T> + Clone,
{
    /// Creates a new `StreamingStep` deserializer
    pub fn new(data_deserializer: ST) -> Self {
        Self {
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
            option_deserializer: OptionDeserializer::new(data_deserializer.clone()),
            data_deserializer,
            phantom_t: PhantomData,
        }
    }
}

impl<T, ST> Deserializer<StreamingStep<T>> for StreamingStepDeserializer<T, ST>
where
    T: Clone,
    ST: Deserializer<T>,
{
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], StreamingStep<T>, E> {
        let (rest, ident) = context("identifier", |input| {
            self.u64_deserializer.deserialize(input)
        })
        .parse(buffer)?;
        match ident {
            0u64 => Ok((rest, StreamingStep::Started)),
            1u64 => context("cursor_data", |input| {
                self.data_deserializer.deserialize(input)
            })
            .map(StreamingStep::Ongoing)
            .parse(rest),
            2u64 => context("last_data", |input| {
                self.option_deserializer.deserialize(input)
            })
            .map(StreamingStep::Finished)
            .parse(rest),
            _ => Err(nom::Err::Error(ParseError::from_error_kind(
                buffer,
                nom::error::ErrorKind::Digit,
            ))),
        }
    }
}
