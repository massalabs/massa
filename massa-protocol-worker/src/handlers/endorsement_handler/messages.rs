use massa_models::{
    endorsement::{Endorsement, EndorsementDeserializer, SecureShareEndorsement},
    secure_share::{SecureShareDeserializer, SecureShareSerializer},
};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ParseError},
    multi::length_count,
    IResult, Parser,
};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use std::ops::Bound::Included;

#[derive(Debug, PartialEq, Eq)]
pub enum EndorsementMessage {
    /// Endorsements
    Endorsements(Vec<SecureShareEndorsement>),
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u64)]
pub enum MessageTypeId {
    Endorsements,
}

impl From<&EndorsementMessage> for MessageTypeId {
    fn from(message: &EndorsementMessage) -> Self {
        match message {
            EndorsementMessage::Endorsements(_) => MessageTypeId::Endorsements,
        }
    }
}

#[derive(Default, Clone)]
pub struct EndorsementMessageSerializer {
    id_serializer: U64VarIntSerializer,
    length_endorsements_serializer: U64VarIntSerializer,
    secure_share_serializer: SecureShareSerializer,
}

impl EndorsementMessageSerializer {
    pub fn new() -> Self {
        Self {
            id_serializer: U64VarIntSerializer::new(),
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
        self.id_serializer.serialize(
            &MessageTypeId::from(value).try_into().map_err(|_| {
                SerializeError::GeneralError(String::from("Failed to serialize id"))
            })?,
            buffer,
        )?;
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

pub struct EndorsementMessageDeserializerArgs {
    pub thread_count: u8,
    pub max_length_endorsements: u64,
    pub endorsement_count: u32,
}

pub struct EndorsementMessageDeserializer {
    id_deserializer: U64VarIntDeserializer,
    length_endorsements_deserializer: U64VarIntDeserializer,
    secure_share_deserializer: SecureShareDeserializer<Endorsement, EndorsementDeserializer>,
}

impl EndorsementMessageDeserializer {
    pub fn new(args: EndorsementMessageDeserializerArgs) -> Self {
        Self {
            id_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
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
}

impl Deserializer<EndorsementMessage> for EndorsementMessageDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], EndorsementMessage, E> {
        context("Failed EndorsementMessage deserialization", |buffer| {
            let (buffer, raw_id) = self.id_deserializer.deserialize(buffer)?;
            let id = MessageTypeId::try_from(raw_id).map_err(|_| {
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

#[cfg(all(test, feature = "testing"))]
mod tests {
    use std::str::FromStr;

    use massa_models::{
        block_id::BlockId,
        endorsement::{Endorsement, EndorsementSerializer},
        secure_share::SecureShareContent,
        slot::Slot,
    };
    use massa_serialization::{DeserializeError, Deserializer, Serializer};
    use massa_signature::KeyPair;

    #[test]
    fn test_lower_limit_message() {
        let message = super::EndorsementMessage::Endorsements(vec![]);
        let mut buffer = Vec::new();
        let serializer = super::EndorsementMessageSerializer::new();
        serializer
            .serialize(&message, &mut buffer)
            .expect("Failed to serialize message");
        let deserializer =
            super::EndorsementMessageDeserializer::new(super::EndorsementMessageDeserializerArgs {
                thread_count: 1,
                max_length_endorsements: 0,
                endorsement_count: 1,
            });
        let (rest, deserialized_message) = deserializer
            .deserialize::<DeserializeError>(&buffer)
            .expect("Failed to deserialize message");
        assert!(rest.is_empty());
        assert_eq!(deserialized_message, message);
    }

    #[test]
    fn test_high_limit_message() {
        let endorsement = Endorsement {
            slot: Slot::new(u64::MAX, 31),
            index: 15,
            endorsed_block: BlockId::from_str(
                "B12DvrcQkzF1Wi8BVoNfc4n93CD3E2qhCNe7nVhnEQGWHZ24fEmg",
            )
            .unwrap(),
        };
        let secure_share_endo = endorsement
            .new_verifiable(EndorsementSerializer::new(), &KeyPair::generate(0).unwrap())
            .unwrap();
        let message = super::EndorsementMessage::Endorsements(vec![
            secure_share_endo.clone(),
            secure_share_endo.clone(),
        ]);
        let mut buffer = Vec::new();
        let serializer = super::EndorsementMessageSerializer::new();
        serializer
            .serialize(&message, &mut buffer)
            .expect("Failed to serialize message");
        let deserializer =
            super::EndorsementMessageDeserializer::new(super::EndorsementMessageDeserializerArgs {
                thread_count: 32,
                max_length_endorsements: 1,
                endorsement_count: 16,
            });
        deserializer
            .deserialize::<DeserializeError>(&buffer)
            .expect_err("Should fail because there is two element and the limit is one");
        let message2 = super::EndorsementMessage::Endorsements(vec![secure_share_endo.clone()]);
        let mut buffer2 = Vec::new();
        serializer
            .serialize(&message2, &mut buffer2)
            .expect("Failed to serialize message");
        let (rest, deserialized_message) = deserializer
            .deserialize::<DeserializeError>(&buffer2)
            .expect("Failed to deserialize message");
        assert!(rest.is_empty());
        assert_eq!(deserialized_message, message2);
    }
}
