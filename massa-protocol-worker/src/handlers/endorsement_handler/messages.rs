use massa_models::{
    endorsement::{Endorsement, EndorsementDeserializer, SecureShareEndorsement},
    secure_share::{SecureShareDeserializer, SecureShareSerializer},
};
use massa_serialization::{Deserializer, Serializer, U64VarIntDeserializer, U64VarIntSerializer};
use nom::{
    error::{context, ContextError, ParseError},
    multi::length_count,
    IResult, Parser,
};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use std::ops::Bound::Included;

#[derive(Debug)]
pub(crate) enum EndorsementMessage {
    /// Endorsements
    Endorsements(Vec<SecureShareEndorsement>),
}

impl EndorsementMessage {
    pub(crate) fn get_id(&self) -> MessageTypeId {
        match self {
            EndorsementMessage::Endorsements(_) => MessageTypeId::Endorsements,
        }
    }

    pub(crate) fn max_id() -> u64 {
        <MessageTypeId as Into<u64>>::into(MessageTypeId::Endorsements) + 1
    }
}

// DO NOT FORGET TO UPDATE MAX ID IF YOU UPDATE THERE
#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u64)]
pub(crate) enum MessageTypeId {
    Endorsements,
}

#[derive(Default, Clone)]
pub(crate) struct EndorsementMessageSerializer {
    length_endorsements_serializer: U64VarIntSerializer,
    secure_share_serializer: SecureShareSerializer,
}

impl EndorsementMessageSerializer {
    pub(crate) fn new() -> Self {
        Self {
            length_endorsements_serializer: U64VarIntSerializer::new(),
            secure_share_serializer: SecureShareSerializer::new(),
        }
    }
}

impl Serializer<EndorsementMessage> for EndorsementMessageSerializer {
    fn serialize(
        &self,
        value: &EndorsementMessage,
        buffer: &mut Vec<u8>,
    ) -> Result<(), massa_serialization::SerializeError> {
        match value {
            EndorsementMessage::Endorsements(endorsements) => {
                self.length_endorsements_serializer
                    .serialize(&(endorsements.len() as u64), buffer)?;
                for endorsement in endorsements {
                    self.secure_share_serializer
                        .serialize(endorsement, buffer)?;
                }
            }
        }
        Ok(())
    }
}

pub(crate) struct EndorsementMessageDeserializerArgs {
    pub(crate) thread_count: u8,
    pub(crate) max_length_endorsements: u64,
    pub(crate) endorsement_count: u32,
}

pub(crate) struct EndorsementMessageDeserializer {
    message_id: u64,
    length_endorsements_deserializer: U64VarIntDeserializer,
    secure_share_deserializer: SecureShareDeserializer<Endorsement, EndorsementDeserializer>,
}

impl EndorsementMessageDeserializer {
    pub(crate) fn new(args: EndorsementMessageDeserializerArgs) -> Self {
        Self {
            message_id: 0,
            length_endorsements_deserializer: U64VarIntDeserializer::new(
                Included(0),
                Included(args.max_length_endorsements),
            ),
            secure_share_deserializer: SecureShareDeserializer::new(EndorsementDeserializer::new(
                args.thread_count,
                args.endorsement_count,
            )),
        }
    }

    pub(crate) fn set_message_id(&mut self, message_id: u64) {
        self.message_id = message_id;
    }
}

impl Deserializer<EndorsementMessage> for EndorsementMessageDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], EndorsementMessage, E> {
        context("Failed EndorsementMessage deserialization", |buffer| {
            let id = MessageTypeId::try_from(self.message_id).map_err(|_| {
                nom::Err::Error(ParseError::from_error_kind(
                    buffer,
                    nom::error::ErrorKind::Eof,
                ))
            })?;
            match id {
                MessageTypeId::Endorsements => context(
                    "Failed Endorsements deserialization",
                    length_count(
                        context("Failed length deserialization", |input| {
                            self.length_endorsements_deserializer.deserialize(input)
                        }),
                        context("Failed endorsement deserialization", |input| {
                            self.secure_share_deserializer.deserialize(input)
                        }),
                    ),
                )
                .map(EndorsementMessage::Endorsements)
                .parse(buffer),
            }
        })
        .parse(buffer)
    }
}
