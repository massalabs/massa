// Copyright (c) 2022 MASSA LABS <info@massa.net>

use nom::bytes::complete::take;
use nom::error::{context, ContextError, ParseError};
use nom::sequence::tuple;
use nom::{IResult, Parser};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::ops::Bound::{Excluded, Included};

use crate::slot::{Slot, SlotDeserializer, SlotSerializer};
// use crate::serialization::VecU8Serializer;
// use crate::timeslots::slot_count_in_range;
use massa_hash::{Hash, HashDeserializer};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U32VarIntDeserializer, U32VarIntSerializer,
};
use massa_signature::{verify_signature_batch, PublicKey, Signature, SignatureDeserializer};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndorsementDenouncement {
    pub signature_1: Signature,
    pub hash_1: Hash,
    pub index_1: u32,
    pub signature_2: Signature,
    pub hash_2: Hash,
    pub index_2: u32,
}

impl EndorsementDenouncement {
    fn is_valid(&self, public_key: PublicKey) -> bool {
        let to_verif = [
            (self.hash_1, self.signature_1, public_key),
            (self.hash_2, self.signature_2, public_key),
        ];

        self.hash_1 == self.hash_2
            && self.index_1 == self.index_2
            && verify_signature_batch(&to_verif).is_ok()
    }
}

impl Display for EndorsementDenouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Endorsement denouncement @ index: {}", self.index_1)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockDenouncement {
    pub signature_1: Signature,
    pub hash_1: Hash,
    pub signature_2: Signature,
    pub hash_2: Hash,
}

impl BlockDenouncement {
    fn is_valid(&self, public_key: PublicKey) -> bool {
        let to_verif = [
            (self.hash_1, self.signature_1, public_key),
            (self.hash_2, self.signature_2, public_key),
        ];

        self.hash_1 == self.hash_2 && verify_signature_batch(&to_verif).is_ok()
    }
}

impl Display for BlockDenouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Block denouncement")?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DenouncementProof {
    Endorsement(EndorsementDenouncement),
    Block(BlockDenouncement),
}

impl AsRef<Self> for DenouncementProof {
    fn as_ref(&self) -> &Self {
        &self
    }
}

impl Display for DenouncementProof {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            DenouncementProof::Endorsement(ed) => {
                writeln!(f, "{}", ed)?;
            }
            DenouncementProof::Block(bd) => {
                writeln!(f, "{}", bd)?;
            }
        }
        Ok(())
    }
}

/// a denouncement, as sent in the network
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Denouncement {
    pub slot: Slot,
    pub proof: DenouncementProof,
}

impl Denouncement {
    fn is_valid(&self, public_key: PublicKey) -> bool {
        match self.proof.as_ref() {
            DenouncementProof::Endorsement(ed) => ed.is_valid(public_key),
            DenouncementProof::Block(bd) => bd.is_valid(public_key),
        }
    }

    fn is_for_block(&self) -> bool {
        matches!(self.proof.as_ref(), DenouncementProof::Block(_))
    }

    fn is_for_endorsement(&self) -> bool {
        matches!(self.proof.as_ref(), DenouncementProof::Endorsement(_))
    }
}

impl Display for Denouncement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Denouncement at slot {}", self.slot)?;
        writeln!(f, "Proof: {}", self.proof)?;
        Ok(())
    }
}

/// Serializer for ``
pub struct DenouncementSerializer {
    u32_serializer: U32VarIntSerializer,
    slot_serializer: SlotSerializer,
}

impl DenouncementSerializer {
    /// Creates a new ``
    pub fn new() -> Self {
        DenouncementSerializer {
            u32_serializer: U32VarIntSerializer::new(),
            slot_serializer: SlotSerializer::new(),
        }
    }
}

/*
impl Default for DenouncementSerializer {
    fn default() -> Self {
        Self::new()
    }
}
*/

impl Serializer<Denouncement> for DenouncementSerializer {
    /// ## Example:
    /// ```rust
    /// use massa_models::{slot::Slot, block::BlockId, endorsement::{Endorsement, EndorsementSerializerLW}};
    /// use massa_serialization::Serializer;
    /// use massa_hash::Hash;
    ///
    /// let endorsement = Endorsement {
    ///   slot: Slot::new(1, 2),
    ///   index: 0,
    ///   endorsed_block: BlockId(Hash::compute_from("test".as_bytes()))
    /// };
    /// let mut buffer = Vec::new();
    /// EndorsementSerializerLW::new().serialize(&endorsement, &mut buffer).unwrap();
    /// ```
    fn serialize(&self, value: &Denouncement, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.slot_serializer.serialize(&value.slot, buffer)?;
        let denouncement_kind = value.is_for_block() as u8;
        buffer.extend([denouncement_kind]);

        match value.proof.as_ref() {
            DenouncementProof::Endorsement(ed) => {
                buffer.extend(ed.signature_1.to_bytes());
                buffer.extend(ed.hash_1.to_bytes());
                self.u32_serializer.serialize(&ed.index_1, buffer)?;
                buffer.extend(ed.signature_2.to_bytes());
                buffer.extend(ed.hash_2.to_bytes());
                self.u32_serializer.serialize(&ed.index_2, buffer)?;
            }
            DenouncementProof::Block(_bd) => {
                todo!()
            }
        }

        Ok(())
    }
}

/// Deserializer for ``
pub struct DenouncementDeserializer {
    slot_deserializer: SlotDeserializer,
    index_deserializer: U32VarIntDeserializer,
    hash_deserializer: HashDeserializer,
    sig_desserializer: SignatureDeserializer,
}

impl DenouncementDeserializer {
    /// Creates a new ``
    pub fn new(thread_count: u8, endorsement_count: u32) -> Self {
        Self {
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            index_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Excluded(endorsement_count),
            ),
            hash_deserializer: HashDeserializer::new(),
            sig_desserializer: SignatureDeserializer::new(),
        }
    }
}

impl Deserializer<Denouncement> for DenouncementDeserializer {
    /// ## Example:
    /// ```rust
    /// use massa_models::{slot::Slot, block::BlockId, endorsement::{Endorsement, EndorsementSerializerLW, EndorsementDeserializerLW}};
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use massa_hash::Hash;
    ///
    /// let slot = Slot::new(1, 2);
    /// let endorsed_block = BlockId(Hash::compute_from("test".as_bytes()));
    /// let endorsement = Endorsement {
    ///   slot: slot,
    ///   index: 0,
    ///   endorsed_block: endorsed_block
    /// };
    /// let mut buffer = Vec::new();
    /// EndorsementSerializerLW::new().serialize(&endorsement, &mut buffer).unwrap();
    /// let (rest, deserialized) = EndorsementDeserializerLW::new(10, slot, endorsed_block).deserialize::<DeserializeError>(&buffer).unwrap();
    /// assert_eq!(rest.len(), 0);
    /// assert_eq!(deserialized.index, endorsement.index);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Denouncement, E> {
        let (rem, (slot, is_for_block)) = context(
            "Failed Denouncement deserialization",
            tuple((
                context("Failed slot deserialization", |input| {
                    self.slot_deserializer.deserialize(input)
                }),
                context("Failed slot deserialization", |input| take(1usize)(input)),
            )),
        )
        .map(|(slot, is_for_block)| (slot, is_for_block))
        .parse(buffer)?;

        // TODO: rework this
        let is_for_block_ = matches!(is_for_block, [1]);

        let (rem2, proof): (_, DenouncementProof) = match is_for_block_ {
            true => {
                todo!()
                /*
                context("Failed Block denouncement deser", |input| {
                    todo!()
                })
                */
            }
            false => context(
                "Failed Endorsement denouncement deser",
                tuple((
                    context("Failed signature 1 deser", |input| {
                        self.sig_desserializer.deserialize(input)
                    }),
                    context("Failed hash 1 deser", |input| {
                        self.hash_deserializer.deserialize(input)
                    }),
                    context("Failed index 1 deser", |input| {
                        self.index_deserializer.deserialize(input)
                    }),
                    context("Failed signature 2 deser", |input| {
                        self.sig_desserializer.deserialize(input)
                    }),
                    context("Failed hash 2 deser", |input| {
                        self.hash_deserializer.deserialize(input)
                    }),
                    context("Failed index 2 deser", |input| {
                        self.index_deserializer.deserialize(input)
                    }),
                )),
            )
            .map(|(sig1, hash1, idx1, sig2, hash2, idx2)| {
                let ed = EndorsementDenouncement {
                    signature_1: sig1,
                    hash_1: hash1,
                    index_1: idx1,
                    signature_2: sig2,
                    hash_2: hash2,
                    index_2: idx2,
                };
                DenouncementProof::Endorsement(ed)
            })
            .parse(rem)?,
        };

        Ok((rem2, Denouncement { slot, proof }))
    }
}

#[cfg(test)]
mod tests {
    // use crate::wrapped::{WrappedDeserializer, WrappedSerializer};

    use super::*;
    use massa_serialization::DeserializeError;
    use serial_test::serial;

    // use massa_serialization::DeserializeError;
    use crate::block::BlockId;
    use crate::endorsement::{
        Endorsement, EndorsementHasher, EndorsementSerializer, WrappedEndorsement,
    };
    use crate::wrapped::{Id, WrappedContent};
    use massa_signature::KeyPair;

    #[test]
    #[serial]
    fn test_endorsement_denouncement() {
        let sender_keypair = KeyPair::generate();

        let slot = Slot::new(3, 7);
        let content = Endorsement {
            slot,
            index: 0,
            endorsed_block: BlockId(Hash::compute_from("blk".as_bytes())),
        };
        let endorsement1: WrappedEndorsement = Endorsement::new_wrapped_with_hasher(
            content.clone(),
            EndorsementSerializer::new(),
            &sender_keypair,
            EndorsementHasher::new(),
        )
        .unwrap();

        let endorsement2: WrappedEndorsement = Endorsement::new_wrapped_with_hasher(
            content,
            EndorsementSerializer::new(),
            &sender_keypair,
            EndorsementHasher::new(),
        )
        .unwrap();

        assert_eq!(endorsement1, endorsement2);

        let denouncement = Denouncement {
            slot,
            proof: DenouncementProof::Endorsement(EndorsementDenouncement {
                signature_1: endorsement1.signature,
                hash_1: *endorsement1.id.get_hash(),
                index_1: endorsement1.content.index,
                signature_2: endorsement2.signature,
                hash_2: *endorsement2.id.get_hash(),
                index_2: endorsement2.content.index,
            }),
        };

        assert_eq!(denouncement.is_valid(sender_keypair.get_public_key()), true);

        let content2 = Endorsement {
            slot,
            index: 1,
            endorsed_block: BlockId(Hash::compute_from("blk".as_bytes())),
        };
        let endorsement3: WrappedEndorsement = Endorsement::new_wrapped_with_hasher(
            content2.clone(),
            EndorsementSerializer::new(),
            &sender_keypair,
            EndorsementHasher::new(),
        )
        .unwrap();

        let denouncement2 = Denouncement {
            slot,
            proof: DenouncementProof::Endorsement(EndorsementDenouncement {
                signature_1: endorsement1.signature,
                hash_1: *endorsement1.id.get_hash(),
                index_1: endorsement1.content.index,
                signature_2: endorsement3.signature,
                hash_2: *endorsement3.id.get_hash(),
                index_2: endorsement1.content.index, // We explicitly use endorsement1 here as we want to test the sig check
            }),
        };

        assert_eq!(
            denouncement2.is_valid(sender_keypair.get_public_key()),
            false
        );
    }

    #[test]
    #[serial]
    fn test_endorsement_denouncement_ser_deser() {
        let sender_keypair = KeyPair::generate();

        let slot = Slot::new(3, 7);
        let content = Endorsement {
            slot,
            index: 0,
            endorsed_block: BlockId(Hash::compute_from("blk".as_bytes())),
        };
        let endorsement1: WrappedEndorsement = Endorsement::new_wrapped_with_hasher(
            content.clone(),
            EndorsementSerializer::new(),
            &sender_keypair,
            EndorsementHasher::new(),
        )
        .unwrap();

        let endorsement2: WrappedEndorsement = Endorsement::new_wrapped_with_hasher(
            content,
            EndorsementSerializer::new(),
            &sender_keypair,
            EndorsementHasher::new(),
        )
        .unwrap();

        let denouncement = Denouncement {
            slot,
            proof: DenouncementProof::Endorsement(EndorsementDenouncement {
                signature_1: endorsement1.signature,
                hash_1: *endorsement1.id.get_hash(),
                index_1: endorsement1.content.index,
                signature_2: endorsement2.signature,
                hash_2: *endorsement2.id.get_hash(),
                index_2: endorsement2.content.index,
            }),
        };

        let mut ser: Vec<u8> = Vec::new();
        let serializer = DenouncementSerializer::new();
        serializer.serialize(&denouncement, &mut ser).unwrap();

        let deserializer = DenouncementDeserializer::new(32, 16);
        let (_, res_denouncement) = deserializer.deserialize::<DeserializeError>(&ser).unwrap();

        assert_eq!(denouncement, res_denouncement);
    }
}
