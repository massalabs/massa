// Copyright (c) 2022 MASSA LABS <info@massa.net>

use nom::bytes::complete::take;
use nom::error::{context, ContextError, ParseError};
use nom::sequence::tuple;
use nom::{IResult, Parser};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::ops::Bound::{Excluded, Included};
use std::str::FromStr;

use crate::slot::{Slot, SlotDeserializer, SlotSerializer};
use massa_hash::{Hash, HashDeserializer};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U32VarIntDeserializer, U32VarIntSerializer,
};
use massa_signature::{verify_signature_batch, PublicKey, Signature, SignatureDeserializer, PublicKeyDeserializer};
use crate::address::Address;
use crate::block::WrappedHeader;
use crate::endorsement::WrappedEndorsement;
use crate::error::ModelsError;
use crate::prehash::PreHashed;
use crate::wrapped::Id;

/// Denunciation ID size in bytes
pub const DENUNCIATION_ID_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;

/// endorsement id
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct DenunciationId(Hash);

impl PreHashed for DenunciationId {}

impl Id for DenunciationId {
    fn new(hash: Hash) -> Self {
        DenunciationId(hash)
    }

    fn get_hash(&self) -> &Hash {
        &self.0
    }
}

impl Display for DenunciationId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0.to_bs58_check())
    }
}

impl FromStr for DenunciationId {
    type Err = ModelsError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(DenunciationId(Hash::from_str(s)?))
    }
}

impl DenunciationId {
    /// endorsement id to bytes
    pub fn to_bytes(&self) -> &[u8; DENUNCIATION_ID_SIZE_BYTES] {
        self.0.to_bytes()
    }

    /// endorsement id into bytes
    pub fn into_bytes(self) -> [u8; DENUNCIATION_ID_SIZE_BYTES] {
        self.0.into_bytes()
    }

    /// endorsement id from bytes
    pub fn from_bytes(data: &[u8; DENUNCIATION_ID_SIZE_BYTES]) -> DenunciationId {
        DenunciationId(Hash::from_bytes(data))
    }

    /// endorsement id from `bs58` check
    pub fn from_bs58_check(data: &str) -> Result<DenunciationId, ModelsError> {
        Ok(DenunciationId(
            Hash::from_bs58_check(data).map_err(|_| ModelsError::HashError)?,
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndorsementDenunciation {
    pub index: u32,
    pub signature_1: Signature,
    pub hash_1: Hash,
    pub signature_2: Signature,
    pub hash_2: Hash,
}

impl EndorsementDenunciation {
    fn is_valid(&self, public_key: PublicKey) -> bool {
        let to_verif = [
            (self.hash_1, self.signature_1, public_key),
            (self.hash_2, self.signature_2, public_key),
        ];

        self.hash_1 != self.hash_2
            && verify_signature_batch(&to_verif).is_ok()
    }
}

impl Display for EndorsementDenunciation {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Endorsement Denunciation @ index: {}", self.index)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockDenunciation {
    pub signature_1: Signature,
    pub hash_1: Hash,
    pub signature_2: Signature,
    pub hash_2: Hash,
}

impl BlockDenunciation {
    fn is_valid(&self, public_key: PublicKey) -> bool {
        let to_verif = [
            (self.hash_1, self.signature_1, public_key),
            (self.hash_2, self.signature_2, public_key),
        ];

        self.hash_1 != self.hash_2
            && verify_signature_batch(&to_verif).is_ok()
    }
}

impl Display for BlockDenunciation {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Block Denunciation")?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DenunciationProof {
    Endorsement(EndorsementDenunciation),
    Block(BlockDenunciation),
}

impl AsRef<Self> for DenunciationProof {
    fn as_ref(&self) -> &Self {
        &self
    }
}

impl Display for DenunciationProof {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            DenunciationProof::Endorsement(ed) => {
                writeln!(f, "{}", ed)?;
            }
            DenunciationProof::Block(bd) => {
                writeln!(f, "{}", bd)?;
            }
        }
        Ok(())
    }
}

/// a Denunciation, as sent in the network
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Denunciation {
    pub slot: Slot,
    pub pub_key: PublicKey,
    pub proof: DenunciationProof,
}

impl Denunciation {
    pub fn is_valid(&self) -> bool {
        match self.proof.as_ref() {
            DenunciationProof::Endorsement(ed) => ed.is_valid(self.pub_key),
            DenunciationProof::Block(bd) => bd.is_valid(self.pub_key),
        }
    }

    pub fn is_for_block(&self) -> bool {
        matches!(self.proof.as_ref(), DenunciationProof::Block(_))
    }

    fn is_for_endorsement(&self) -> bool {
        matches!(self.proof.as_ref(), DenunciationProof::Endorsement(_))
    }

    pub fn from_wrapped_endorsements(e1: &WrappedEndorsement, e2: &WrappedEndorsement) -> Self {

        // FIXME: Should we return a Result and only forge valid Denunciation?
        Self {
            slot: e1.content.slot,
            pub_key: e1.creator_public_key,
            proof: DenunciationProof::Endorsement(EndorsementDenunciation {
                index: e1.content.index,
                signature_1: e1.signature,
                hash_1: e1.id.get_hash().clone(),
                signature_2: e2.signature,
                hash_2: e2.id.get_hash().clone(),
            })
        }
    }

    pub fn from_wrapped_headers(h1: &WrappedHeader, h2: &WrappedHeader) -> Self {

        // FIXME: Should we return a Result and only forge valid Denunciation?
        Self {
            slot: h1.content.slot,
            pub_key: h1.creator_public_key,
            proof: DenunciationProof::Block(BlockDenunciation {
                signature_1: h1.signature,
                hash_1: h1.id.get_hash().clone(),
                signature_2: h2.signature,
                hash_2: h2.id.get_hash().clone(),
            })
        }
    }

    /// Address of the denounced
    pub fn addr(&self) -> Address {
        Address::from_public_key(&self.pub_key)
    }

    pub fn get_id<SC>(&self, serializer: SC) -> Result<DenunciationId, SerializeError>
        where SC: Serializer<Self>
    {
        let mut buffer = Vec::new();
        serializer.serialize(self, &mut buffer)?;
        let hash = Hash::compute_from(&buffer[..]);
        Ok(DenunciationId::new(hash))
    }
}

impl Display for Denunciation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Denunciation at slot {}", self.slot)?;
        writeln!(f, "Proof: {}", self.proof)?;
        Ok(())
    }
}

/// Serializer for ``
pub struct DenunciationSerializer {
    u32_serializer: U32VarIntSerializer,
    slot_serializer: SlotSerializer,
}

impl DenunciationSerializer {
    /// Creates a new ``
    pub fn new() -> Self {
        DenunciationSerializer {
            u32_serializer: U32VarIntSerializer::new(),
            slot_serializer: SlotSerializer::new(),
        }
    }
}

/*
impl Default for DenunciationSerializer {
    fn default() -> Self {
        Self::new()
    }
}
*/

impl Serializer<Denunciation> for DenunciationSerializer {
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
    fn serialize(&self, value: &Denunciation, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.slot_serializer.serialize(&value.slot, buffer)?;
        buffer.extend(value.pub_key.to_bytes());
        let denunciation_kind = value.is_for_block() as u8;
        buffer.extend([denunciation_kind]);

        match value.proof.as_ref() {
            DenunciationProof::Endorsement(ed) => {
                self.u32_serializer.serialize(&ed.index, buffer)?;
                buffer.extend(ed.signature_1.to_bytes());
                buffer.extend(ed.hash_1.to_bytes());
                buffer.extend(ed.signature_2.to_bytes());
                buffer.extend(ed.hash_2.to_bytes());
            }
            DenunciationProof::Block(_bd) => {
                todo!()
            }
        }

        Ok(())
    }
}

/// Deserializer for ``
pub struct DenunciationDeserializer {
    slot_deserializer: SlotDeserializer,
    index_deserializer: U32VarIntDeserializer,
    hash_deserializer: HashDeserializer,
    sig_deserializer: SignatureDeserializer,
    pub_key_deserializer: PublicKeyDeserializer,
}

impl DenunciationDeserializer {
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
            sig_deserializer: SignatureDeserializer::new(),
            pub_key_deserializer: PublicKeyDeserializer::new(),
        }
    }
}

impl Deserializer<Denunciation> for DenunciationDeserializer {
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
    ) -> IResult<&'a [u8], Denunciation, E> {
        let (rem, (slot, pub_key, is_for_block)) = context(
            "Failed Denunciation deserialization",
            tuple((
                context("Failed slot deserialization", |input| {
                    self.slot_deserializer.deserialize(input)
                }),
                context("Failed pub_key deserialization", |input| {
                    self.pub_key_deserializer.deserialize(input)
                }),
                context("Failed slot deserialization", |input| take(1usize)(input)),
            )),
        )
        .parse(buffer)?;

        // TODO: rework this
        let is_for_block_ = matches!(is_for_block, [1]);

        let (rem2, proof): (_, DenunciationProof) = match is_for_block_ {
            true => {
                todo!()
                /*
                context("Failed Block Denunciation deser", |input| {
                    todo!()
                })
                */
            }
            false => context(
                "Failed Endorsement Denunciation deser",
                tuple((
                    context("Failed index deser", |input| {
                        self.index_deserializer.deserialize(input)
                    }),
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
            )
            .map(|(idx, sig1, hash1, sig2, hash2)| {
                let ed = EndorsementDenunciation {
                    index: idx,
                    signature_1: sig1,
                    hash_1: hash1,
                    signature_2: sig2,
                    hash_2: hash2,
                };
                DenunciationProof::Endorsement(ed)
            })
            .parse(rem)?,
        };

        Ok((rem2, Denunciation { slot, pub_key, proof }))
    }
}

#[cfg(test)]
mod tests {
    // use crate::wrapped::{WrappedDeserializer, WrappedSerializer};

    use super::*;
    use massa_serialization::DeserializeError;
    use serial_test::serial;

    // use massa_serialization::DeserializeError;
    use crate::block::{Block, BlockHeader, BlockHeaderSerializer, BlockId, WrappedHeader};
    use crate::endorsement::{Endorsement, EndorsementHasher, EndorsementSerializer, EndorsementSerializerLW, WrappedEndorsement};
    use crate::wrapped::{Id, Wrapped, WrappedContent};
    use massa_signature::KeyPair;
    use crate::config::THREAD_COUNT;

    #[test]
    #[serial]
    fn test_endorsement_denunciation() {
        let sender_keypair = KeyPair::generate();

        let slot = Slot::new(3, 7);
        let content = Endorsement {
            slot,
            index: 0,
            endorsed_block: BlockId(Hash::compute_from("blk1".as_bytes())),
        };
        let endorsement1: WrappedEndorsement = Endorsement::new_wrapped_with_hasher(
            content.clone(),
            EndorsementSerializer::new(),
            &sender_keypair,
            EndorsementHasher::new(),
        )
        .unwrap();

        let content2 = Endorsement {
            slot,
            index: 0,
            endorsed_block: BlockId(Hash::compute_from("blk2".as_bytes())),
        };
        let endorsement2: WrappedEndorsement = Endorsement::new_wrapped_with_hasher(
            content2,
            EndorsementSerializer::new(),
            &sender_keypair,
            EndorsementHasher::new(),
        )
        .unwrap();

        let denunciation = Denunciation {
            slot,
            pub_key: sender_keypair.get_public_key(),
            proof: DenunciationProof::Endorsement(EndorsementDenunciation {
                index: endorsement1.content.index,
                signature_1: endorsement1.signature,
                hash_1: *endorsement1.id.get_hash(),
                signature_2: endorsement2.signature,
                hash_2: *endorsement2.id.get_hash(),
            }),
        };

        assert_eq!(denunciation.is_valid(), true);

    }

    #[test]
    #[serial]
    fn test_invalid_endorsement_denunciation() {

        let sender_keypair = KeyPair::generate();

        let slot = Slot::new(3, 7);

        let content = Endorsement {
            slot,
            index: 1,
            endorsed_block: BlockId(Hash::compute_from("blk".as_bytes())),
        };
        let endorsement1: WrappedEndorsement = Endorsement::new_wrapped_with_hasher(
            content.clone(),
            EndorsementSerializer::new(),
            &sender_keypair,
            EndorsementHasher::new(),
        ).unwrap();
        let endorsement2: WrappedEndorsement = Endorsement::new_wrapped_with_hasher(
            content.clone(),
            EndorsementSerializer::new(),
            &sender_keypair,
            EndorsementHasher::new(),
        ).unwrap();

        // Here we create a Denunciation that report the same block - this is invalid
        let denunciation = Denunciation {
            slot,
            pub_key: sender_keypair.get_public_key(),
            proof: DenunciationProof::Endorsement(EndorsementDenunciation {
                index: endorsement1.content.index,
                signature_1: endorsement1.signature,
                hash_1: *endorsement1.id.get_hash(),
                signature_2: endorsement2.signature,
                hash_2: *endorsement2.id.get_hash(),
            }),
        };

        assert_eq!(
            denunciation.is_valid(),
            false
        );
    }

    #[test]
    #[serial]
    fn test_endorsement_denunciation_ser_deser() {
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

        let content2 = Endorsement {
            slot,
            index: 0,
            endorsed_block: BlockId(Hash::compute_from("blk2".as_bytes())),
        };
        let endorsement2: WrappedEndorsement = Endorsement::new_wrapped_with_hasher(
            content2,
            EndorsementSerializer::new(),
            &sender_keypair,
            EndorsementHasher::new(),
        )
        .unwrap();

        let denunciation = Denunciation {
            slot,
            pub_key: sender_keypair.get_public_key(),
            proof: DenunciationProof::Endorsement(EndorsementDenunciation {
                index: endorsement1.content.index,
                signature_1: endorsement1.signature,
                hash_1: *endorsement1.id.get_hash(),
                signature_2: endorsement2.signature,
                hash_2: *endorsement2.id.get_hash(),
            }),
        };

        assert_eq!(denunciation.is_valid(), true);

        let mut ser: Vec<u8> = Vec::new();
        let serializer = DenunciationSerializer::new();
        serializer.serialize(&denunciation, &mut ser).unwrap();

        let deserializer = DenunciationDeserializer::new(32, 16);
        let (_, res_denunciation) = deserializer.deserialize::<DeserializeError>(&ser).unwrap();

        assert_eq!(denunciation, res_denunciation);
    }

    #[test]
    #[serial]
    fn test_block_denunciation() {

        let keypair = KeyPair::generate();

        let slot = Slot::new(2, 1);
        let parents: Vec<BlockId> = (0..THREAD_COUNT)
            .map(|i| BlockId(Hash::compute_from(&[i])))
            .collect();

        let parents2: Vec<BlockId> = (0..THREAD_COUNT)
            .map(|i| BlockId(Hash::compute_from(&[i+1])))
            .collect();

        let header1 = BlockHeader {
            slot,
            parents: parents.clone(),
            operation_merkle_root: Hash::compute_from("mno".as_bytes()),
            endorsements: vec![
                Endorsement::new_wrapped(
                    Endorsement {
                        slot: Slot::new(1, 1),
                        index: 1,
                        endorsed_block: BlockId(Hash::compute_from("blk1".as_bytes())),
                    },
                    EndorsementSerializerLW::new(),
                    &keypair,
                )
                    .unwrap(),
            ],
        };

        let wrapped_header1: Wrapped<BlockHeader, BlockId> = BlockHeader::new_wrapped(
            header1,
            BlockHeaderSerializer::new(),
            &keypair
        ).unwrap();

        let header2 = BlockHeader {
            slot,
            parents: parents2,
            operation_merkle_root: Hash::compute_from("mno".as_bytes()),
            endorsements: vec![
                Endorsement::new_wrapped(
                    Endorsement {
                        slot: Slot::new(1, 1),
                        index: 1,
                        endorsed_block: BlockId(Hash::compute_from("blk1".as_bytes())),
                    },
                    EndorsementSerializerLW::new(),
                    &keypair,
                )
                    .unwrap(),
            ],
        };

        let wrapped_header2: Wrapped<BlockHeader, BlockId> = BlockHeader::new_wrapped(
            header2,
            BlockHeaderSerializer::new(),
            &keypair
        ).unwrap();

        let denunciation = Denunciation {
            slot,
            pub_key: keypair.get_public_key(),
            proof: DenunciationProof::Block(BlockDenunciation {
                signature_1: wrapped_header1.signature,
                hash_1: *wrapped_header1.id.get_hash(),
                signature_2: wrapped_header2.signature,
                hash_2: *wrapped_header2.id.get_hash(),
            }),
        };

        assert_eq!(denunciation.is_valid(), true);
    }
}
