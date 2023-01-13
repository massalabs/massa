use nom::bytes::complete::take;
use nom::error::{context, ContextError, ParseError};
use nom::sequence::tuple;
use nom::{IResult, Parser};
use serde::{Deserialize, Serialize};
use std::ops::Bound::{Excluded, Included};
// use massa_hash::HashDeserializer;

use massa_serialization::{
    Deserializer,
    SerializeError,
    Serializer,
    // U32VarIntDeserializer, U32VarIntSerializer
};
use massa_signature::{
    PublicKey,
    PublicKeyDeserializer,
    // SignatureDeserializer
};

use crate::slot::{Slot, SlotDeserializer, SlotSerializer};

/// a Denunciation, as sent in the network
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Denunciation {
    /// Slot of denounced objects (either endorsements or blocks)
    pub slot: Slot,
    /// Public key of endorsements/blocks creator
    pub pub_key: PublicKey,
    /// Proof (so everyone can verify that the denunciation is valid)
    pub proof: bool, // dummy
}

/// Serializer for `Denunciation`
pub struct DenunciationSerializer {
    // u32_serializer: U32VarIntSerializer,
    slot_serializer: SlotSerializer,
}

impl DenunciationSerializer {
    /// Creates a new `DenunciationSerializer`
    pub fn new() -> Self {
        DenunciationSerializer {
            // u32_serializer: U32VarIntSerializer::new(),
            slot_serializer: SlotSerializer::new(),
        }
    }
}

impl Default for DenunciationSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<Denunciation> for DenunciationSerializer {
    fn serialize(&self, value: &Denunciation, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.slot_serializer.serialize(&value.slot, buffer)?;
        buffer.extend(value.pub_key.to_bytes());
        // buffer.extend(value.proof.)
        buffer.push(u8::from(value.proof));

        /*
        let denunciation_kind = value.is_for_block() as u8;
        buffer.push(denunciation_kind);

        match value.proof.as_ref() {
            DenunciationProof::Endorsement(ed) => {
                self.u32_serializer.serialize(&ed.index, buffer)?;
                buffer.extend(ed.signature_1.to_bytes());
                buffer.extend(ed.hash_1.to_bytes());
                buffer.extend(ed.signature_2.to_bytes());
                buffer.extend(ed.hash_2.to_bytes());
            }
            DenunciationProof::Block(bd) => {
                buffer.extend(bd.signature_1.to_bytes());
                buffer.extend(bd.hash_1.to_bytes());
                buffer.extend(bd.signature_2.to_bytes());
                buffer.extend(bd.hash_2.to_bytes());
            }
        }
        */

        Ok(())
    }
}

/// Deserializer for Denunciation
pub struct DenunciationDeserializer {
    slot_deserializer: SlotDeserializer,
    //index_deserializer: U32VarIntDeserializer,
    //hash_deserializer: HashDeserializer,
    //sig_deserializer: SignatureDeserializer,
    pub_key_deserializer: PublicKeyDeserializer,
}

impl DenunciationDeserializer {
    /// Creates a new `DenunciationDeserializer`
    pub fn new(thread_count: u8, _endorsement_count: u32) -> Self {
        Self {
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            // index_deserializer: U32VarIntDeserializer::new(
            //     Included(0),
            //     Excluded(endorsement_count),
            // ),
            // hash_deserializer: HashDeserializer::new(),
            // sig_deserializer: SignatureDeserializer::new(),
            pub_key_deserializer: PublicKeyDeserializer::new(),
        }
    }
}

impl Deserializer<Denunciation> for DenunciationDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Denunciation, E> {
        let (rem, (slot, pub_key, proof)) = context(
            "Failed Denunciation deserialization",
            tuple((
                context("Failed slot deserialization", |input| {
                    self.slot_deserializer.deserialize(input)
                }),
                context("Failed pub_key deserialization", |input| {
                    self.pub_key_deserializer.deserialize(input)
                }),
                context("Failed denunciation kind deserialization", |input| {
                    take(1usize)(input)
                }),
            )),
        )
        .parse(buffer)?;

        /*
        let de_kind = DenunciationKind::try_from(is_for_block[0]).map_err(|_| {
            nom::Err::Error(ParseError::from_error_kind(
                buffer,
                nom::error::ErrorKind::Eof
            ))
        })?;

        let (rem2, idx) = match de_kind {
            DenunciationKind::Endorsement => {
                context(
                    "Failed Endorsement Denunciation deser",
                    context("Failed index deser", | input| {
                        self.index_deserializer.deserialize(input)
                    }),
                ).parse(rem)?
            },
            _ => (rem, 0),
        };

        let (rem3, (sig1, hash1, sig2, hash2)) = context(
            "Failed Block Denunciation deser",
            tuple((
                context("Failed signature 1 deser", |input| {
                    self.sig_deserializer.deserialize(input)
                }),
                context("Failed hash 1 deser", |input| {
                    self.hash_deserializer.deserialize(input)
                }),
                context("Failed signature 2 deser", |input| {
                    self.sig_deserializer.deserialize(input)
                }),
                context("Failed hash 2 deser", |input| {
                    self.hash_deserializer.deserialize(input)
                }),
            )),
        ).parse(rem2)?;

        let proof= match de_kind {
            DenunciationKind::Endorsement => {
                let ed = EndorsementDenunciation {
                    index: idx,
                    signature_1: sig1,
                    hash_1: hash1,
                    signature_2: sig2,
                    hash_2: hash2,
                };
                DenunciationProof::Endorsement(ed)
            },
            DenunciationKind::Block => {
                let bd = BlockDenunciation {
                    signature_1: sig1,
                    hash_1: hash1,
                    signature_2: sig2,
                    hash_2: hash2,
                };
                DenunciationProof::Block(bd)
            }
        };
        */

        let proof_ = match proof[0] {
            0 => false,
            _ => true,
        };
        Ok((
            rem,
            Denunciation {
                slot,
                pub_key,
                proof: proof_,
            },
        ))
    }
}
