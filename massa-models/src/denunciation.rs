// Copyright (c) 2022 MASSA LABS <info@massa.net>
/// An overview of what is a Denunciation and what it is used for can be found here
/// https://github.com/massalabs/massa/discussions/3113
use std::cmp::Ordering;
use std::ops::Bound::{Excluded, Included};

use nom::{
    error::{context, ContextError, ParseError},
    sequence::tuple,
    IResult, Parser,
};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::block_header::{BlockHeaderDenunciationData, SecuredHeader};
use crate::endorsement::{EndorsementDenunciationData, SecureShareEndorsement};
use crate::slot::{Slot, SlotDeserializer, SlotSerializer};

use crate::secure_share::Id;
use massa_hash::{Hash, HashDeserializer, HashSerializer};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U32VarIntDeserializer, U32VarIntSerializer,
};
use massa_signature::{
    MassaSignatureError, PublicKey, PublicKeyDeserializer, Signature, SignatureDeserializer,
};

/// A Variant of Denunciation enum for endorsement
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EndorsementDenunciation {
    public_key: PublicKey,
    slot: Slot,
    index: u32,
    hash_1: Hash,
    hash_2: Hash,
    signature_1: Signature,
    signature_2: Signature,
}

impl EndorsementDenunciation {
    /// Rebuild full hash of SecureShareEndorsement from given arguments
    fn compute_hash_for_sig_verif(
        public_key: &PublicKey,
        slot: &Slot,
        index: &u32,
        content_hash: &Hash,
    ) -> Hash {
        let mut hash_data = Vec::new();
        // Public key
        hash_data.extend(public_key.to_bytes());
        // Ser slot & index
        let denunciation_data = EndorsementDenunciationData::new(*slot, *index);
        hash_data.extend(&denunciation_data.to_bytes());
        // Add content hash
        hash_data.extend(content_hash.to_bytes());
        Hash::compute_from(&hash_data)
    }
}

/// A Variant of Denunciation enum for block header
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockHeaderDenunciation {
    public_key: PublicKey,
    slot: Slot,
    hash_1: Hash,
    hash_2: Hash,
    signature_1: Signature,
    signature_2: Signature,
}

impl BlockHeaderDenunciation {
    /// Rebuild full hash of SecuredHeader from given arguments
    fn compute_hash_for_sig_verif(
        public_key: &PublicKey,
        slot: &Slot,
        content_hash: &Hash,
    ) -> Hash {
        let mut hash_data = Vec::new();
        // Public key
        hash_data.extend(public_key.to_bytes());
        // Ser slot
        let de_data = BlockHeaderDenunciationData::new(*slot);
        hash_data.extend(de_data.to_bytes());
        // Add content hash
        hash_data.extend(content_hash.to_bytes());
        Hash::compute_from(&hash_data)
    }
}

/// A denunciation enum
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(missing_docs)]
pub enum Denunciation {
    Endorsement(EndorsementDenunciation),
    BlockHeader(BlockHeaderDenunciation),
}

#[allow(dead_code)]
impl Denunciation {
    /// Check if it is a Denunciation of several endorsements
    pub fn is_for_endorsement(&self) -> bool {
        matches!(self, Denunciation::Endorsement(_))
    }

    /// Check if it is a Denunciation of several block headers
    pub fn is_for_block_header(&self) -> bool {
        matches!(self, Denunciation::BlockHeader(_))
    }

    /// Check if it is a Denunciation for this endorsement
    pub fn is_also_for_endorsement(
        &self,
        s_endorsement: &SecureShareEndorsement,
    ) -> Result<bool, DenunciationError> {
        match self {
            Denunciation::BlockHeader(_) => Ok(false),
            Denunciation::Endorsement(endo_de) => {
                let content_hash = s_endorsement.id.get_hash();

                let hash = EndorsementDenunciation::compute_hash_for_sig_verif(
                    &endo_de.public_key,
                    &endo_de.slot,
                    &endo_de.index,
                    content_hash,
                );

                Ok(endo_de.slot == s_endorsement.content.slot
                    && endo_de.index == s_endorsement.content.index
                    && endo_de.public_key == s_endorsement.content_creator_pub_key
                    && endo_de.hash_1 != *content_hash
                    && endo_de.hash_2 != *content_hash
                    && endo_de
                        .public_key
                        .verify_signature(&hash, &s_endorsement.signature)
                        .is_ok())
            }
        }
    }

    /// Check if it is a Denunciation for this block header
    pub fn is_also_for_block_header(
        &self,
        s_block_header: &SecuredHeader,
    ) -> Result<bool, DenunciationError> {
        match self {
            Denunciation::Endorsement(_) => Ok(false),
            Denunciation::BlockHeader(endo_bh) => {
                let content_hash = s_block_header.id.get_hash();

                let hash = BlockHeaderDenunciation::compute_hash_for_sig_verif(
                    &endo_bh.public_key,
                    &endo_bh.slot,
                    &content_hash,
                );

                Ok(endo_bh.slot == s_block_header.content.slot
                    && endo_bh.public_key == s_block_header.content_creator_pub_key
                    && endo_bh.hash_1 != *content_hash
                    && endo_bh.hash_2 != *content_hash
                    && endo_bh
                        .public_key
                        .verify_signature(&hash, &s_block_header.signature)
                        .is_ok())
            }
        }
    }

    /// Check if Denunciation is valid
    /// Should be used if received from the network (prevent against invalid or attacker crafted denunciation)
    pub fn is_valid(&self) -> bool {
        let (signature_1, signature_2, hash_1, hash_2, public_key) = match self {
            Denunciation::Endorsement(de) => {
                let hash_1 = EndorsementDenunciation::compute_hash_for_sig_verif(
                    &de.public_key,
                    &de.slot,
                    &de.index,
                    &de.hash_1,
                );
                let hash_2 = EndorsementDenunciation::compute_hash_for_sig_verif(
                    &de.public_key,
                    &de.slot,
                    &de.index,
                    &de.hash_2,
                );

                (
                    de.signature_1,
                    de.signature_2,
                    hash_1,
                    hash_2,
                    de.public_key,
                )
            }
            Denunciation::BlockHeader(de) => {
                let hash_1 = BlockHeaderDenunciation::compute_hash_for_sig_verif(
                    &de.public_key,
                    &de.slot,
                    &de.hash_1,
                );
                let hash_2 = BlockHeaderDenunciation::compute_hash_for_sig_verif(
                    &de.public_key,
                    &de.slot,
                    &de.hash_2,
                );

                (
                    de.signature_1,
                    de.signature_2,
                    hash_1,
                    hash_2,
                    de.public_key,
                )
            }
        };

        hash_1 != hash_2
            && public_key.verify_signature(&hash_1, &signature_1).is_ok()
            && public_key.verify_signature(&hash_2, &signature_2).is_ok()
    }

    /// Get Denunciation slot ref
    pub fn get_slot(&self) -> &Slot {
        match self {
            Denunciation::Endorsement(de) => &de.slot,
            Denunciation::BlockHeader(de) => &de.slot,
        }
    }

    /// Get Denunciation public key ref
    pub fn get_public_key(&self) -> &PublicKey {
        match self {
            Denunciation::Endorsement(de) => &de.public_key,
            Denunciation::BlockHeader(de) => &de.public_key,
        }
    }

    /// Check if denunciation has expired given a slot period
    /// Note that slot_period can be:
    /// * A final slot period (for example in order to cleanup denunciation pool caches)
    /// * A block slot period (in execution (execute_denunciation(...)))
    pub fn is_expired(
        denunciation_slot_period: &u64,
        slot_period: &u64,
        denunciation_expire_periods: &u64,
    ) -> bool {
        slot_period.checked_sub(*denunciation_slot_period) > Some(*denunciation_expire_periods)
    }
}

/// Create a new Denunciation from 2 SecureShareEndorsement
impl TryFrom<(&SecureShareEndorsement, &SecureShareEndorsement)> for Denunciation {
    type Error = DenunciationError;

    fn try_from(
        (s_e1, s_e2): (&SecureShareEndorsement, &SecureShareEndorsement),
    ) -> Result<Self, Self::Error> {
        // Cannot use the same endorsement twice
        if s_e1.id == s_e2.id {
            return Err(DenunciationError::InvalidInput);
        }

        // In order to create a Denunciation, there should be the same
        // slot, index & public key
        if s_e1.content.slot != s_e2.content.slot
            || s_e1.content.index != s_e2.content.index
            || s_e1.content_creator_pub_key != s_e2.content_creator_pub_key
            || s_e1.id == s_e2.id
        {
            return Err(DenunciationError::InvalidInput);
        }

        // Check sig of s_e1 with s_e1.public_key, s_e1.slot, s_e1.index
        let s_e1_hash_content = s_e1.id.get_hash();
        let s_e1_hash = EndorsementDenunciation::compute_hash_for_sig_verif(
            &s_e1.content_creator_pub_key,
            &s_e1.content.slot,
            &s_e1.content.index,
            s_e1_hash_content,
        );
        // Check sig of s_e2 but with s_e1.public_key, s_e1.slot, s_e1.index
        let s_e2_hash_content = s_e2.id.get_hash();
        let s_e2_hash = EndorsementDenunciation::compute_hash_for_sig_verif(
            &s_e1.content_creator_pub_key,
            &s_e1.content.slot,
            &s_e1.content.index,
            s_e2_hash_content,
        );

        s_e1.content_creator_pub_key
            .verify_signature(&s_e1_hash, &s_e1.signature)?;
        s_e1.content_creator_pub_key
            .verify_signature(&s_e2_hash, &s_e2.signature)?;

        Ok(Denunciation::Endorsement(EndorsementDenunciation {
            public_key: s_e1.content_creator_pub_key,
            slot: s_e1.content.slot,
            index: s_e1.content.index,
            signature_1: s_e1.signature,
            signature_2: s_e2.signature,
            hash_1: *s_e1_hash_content,
            hash_2: *s_e2_hash_content,
        }))
    }
}

/// Create a new Denunciation from 2 SecureHeader
impl TryFrom<(&SecuredHeader, &SecuredHeader)> for Denunciation {
    type Error = DenunciationError;

    fn try_from((s_bh1, s_bh2): (&SecuredHeader, &SecuredHeader)) -> Result<Self, Self::Error> {
        // Cannot use the same block header twice
        // In order to create a Denunciation, there should be the same slot & public key
        if s_bh1.content.slot != s_bh2.content.slot
            || s_bh1.content_creator_pub_key != s_bh2.content_creator_pub_key
            || s_bh1.id == s_bh2.id
        {
            return Err(DenunciationError::InvalidInput);
        }

        // Check sig of s_bh2 but with s_bh1.public_key, s_bh1.slot, s_bh1.index
        let s_bh1_hash_content = s_bh1.id.get_hash();
        let s_bh1_hash = BlockHeaderDenunciation::compute_hash_for_sig_verif(
            &s_bh1.content_creator_pub_key,
            &s_bh1.content.slot,
            &s_bh1_hash_content,
        );
        let s_bh2_hash_content = s_bh2.id.get_hash();
        let s_bh2_hash = BlockHeaderDenunciation::compute_hash_for_sig_verif(
            &s_bh1.content_creator_pub_key,
            &s_bh1.content.slot,
            &s_bh2_hash_content,
        );

        s_bh1
            .content_creator_pub_key
            .verify_signature(&s_bh1_hash, &s_bh1.signature)?;
        s_bh1
            .content_creator_pub_key
            .verify_signature(&s_bh2_hash, &s_bh2.signature)?;

        Ok(Denunciation::BlockHeader(BlockHeaderDenunciation {
            public_key: s_bh1.content_creator_pub_key,
            slot: s_bh1.content.slot,
            signature_1: s_bh1.signature,
            signature_2: s_bh2.signature,
            hash_1: *s_bh1_hash_content,
            hash_2: *s_bh2_hash_content,
        }))
    }
}

#[allow(missing_docs)]
#[derive(IntoPrimitive, Debug, TryFromPrimitive)]
#[repr(u32)]
pub enum DenunciationTypeId {
    Endorsement = 0,
    BlockHeader = 1,
}

impl From<&Denunciation> for DenunciationTypeId {
    fn from(value: &Denunciation) -> Self {
        match value {
            Denunciation::Endorsement(_) => DenunciationTypeId::Endorsement,
            Denunciation::BlockHeader(_) => DenunciationTypeId::BlockHeader,
        }
    }
}

/// Denunciation error
#[allow(missing_docs)]
#[derive(Error, Debug)]
pub enum DenunciationError {
    #[error("Invalid endorsements or block headers, cannot create denunciation")]
    InvalidInput,
    #[error("signature error: {0}")]
    Signature(#[from] MassaSignatureError),
    #[error("serialization error: {0}")]
    Serialization(#[from] SerializeError),
}

// Serialization / Deserialization

/// Serializer for `EndorsementDenunciation`
struct EndorsementDenunciationSerializer {
    slot_serializer: SlotSerializer,
    u32_serializer: U32VarIntSerializer,
    hash_serializer: HashSerializer,
}

impl EndorsementDenunciationSerializer {
    /// Creates a new `EndorsementDenunciationSerializer`
    const fn new() -> Self {
        Self {
            slot_serializer: SlotSerializer::new(),
            u32_serializer: U32VarIntSerializer::new(),
            hash_serializer: HashSerializer::new(),
        }
    }
}

impl Default for EndorsementDenunciationSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<EndorsementDenunciation> for EndorsementDenunciationSerializer {
    fn serialize(
        &self,
        value: &EndorsementDenunciation,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        buffer.extend(value.public_key.to_bytes());
        self.slot_serializer.serialize(&value.slot, buffer)?;
        self.u32_serializer.serialize(&value.index, buffer)?;
        self.hash_serializer.serialize(&value.hash_1, buffer)?;
        self.hash_serializer.serialize(&value.hash_2, buffer)?;
        buffer.extend(value.signature_1.to_bytes());
        buffer.extend(value.signature_2.to_bytes());
        Ok(())
    }
}

/// Deserializer for `EndorsementDenunciation`
struct EndorsementDenunciationDeserializer {
    slot_deserializer: SlotDeserializer,
    index_deserializer: U32VarIntDeserializer,
    hash_deserializer: HashDeserializer,
    pubkey_deserializer: PublicKeyDeserializer,
    signature_deserializer: SignatureDeserializer,
}

impl EndorsementDenunciationDeserializer {
    /// Creates a new `EndorsementDeserializer`
    const fn new(thread_count: u8, endorsement_count: u32) -> Self {
        EndorsementDenunciationDeserializer {
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            index_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Excluded(endorsement_count),
            ),
            hash_deserializer: HashDeserializer::new(),
            pubkey_deserializer: PublicKeyDeserializer::new(),
            signature_deserializer: SignatureDeserializer::new(),
        }
    }
}

impl Deserializer<EndorsementDenunciation> for EndorsementDenunciationDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], EndorsementDenunciation, E> {
        context(
            "Failed Endorsement Denunciation deserialization",
            tuple((
                context("Failed public key deserialization", |input| {
                    self.pubkey_deserializer.deserialize(input)
                }),
                context("Failed slot deserialization", |input| {
                    self.slot_deserializer.deserialize(input)
                }),
                context("Failed slot deserialization", |input| {
                    self.index_deserializer.deserialize(input)
                }),
                context("Failed hash 1 deserialization", |input| {
                    self.hash_deserializer.deserialize(input)
                }),
                context("Failed hash 2 deserialization", |input| {
                    self.hash_deserializer.deserialize(input)
                }),
                context("Failed signature 1 deserialization", |input| {
                    self.signature_deserializer.deserialize(input)
                }),
                context("Failed signature 2 deserialization", |input| {
                    self.signature_deserializer.deserialize(input)
                }),
            )),
        )
        .map(
            |(public_key, slot, index, hash_1, hash_2, signature_1, signature_2)| {
                EndorsementDenunciation {
                    public_key,
                    slot,
                    index,
                    hash_1,
                    hash_2,
                    signature_1,
                    signature_2,
                }
            },
        )
        .parse(buffer)
    }
}

/// Serializer for `BlockHeaderDenunciation`
struct BlockHeaderDenunciationSerializer {
    slot_serializer: SlotSerializer,
    hash_serializer: HashSerializer,
}

impl BlockHeaderDenunciationSerializer {
    /// Creates a new `BlockHeaderDenunciationSerializer`
    const fn new() -> Self {
        Self {
            slot_serializer: SlotSerializer::new(),
            hash_serializer: HashSerializer::new(),
        }
    }
}

impl Default for BlockHeaderDenunciationSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<BlockHeaderDenunciation> for BlockHeaderDenunciationSerializer {
    fn serialize(
        &self,
        value: &BlockHeaderDenunciation,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        buffer.extend(value.public_key.to_bytes());
        self.slot_serializer.serialize(&value.slot, buffer)?;
        self.hash_serializer.serialize(&value.hash_1, buffer)?;
        self.hash_serializer.serialize(&value.hash_2, buffer)?;
        buffer.extend(value.signature_1.to_bytes());
        buffer.extend(value.signature_2.to_bytes());
        Ok(())
    }
}

/// Deserializer for `BlockHeaderDenunciation`
struct BlockHeaderDenunciationDeserializer {
    slot_deserializer: SlotDeserializer,
    hash_deserializer: HashDeserializer,
    pubkey_deserializer: PublicKeyDeserializer,
    signature_deserializer: SignatureDeserializer,
}

impl BlockHeaderDenunciationDeserializer {
    /// Creates a new `BlockHeaderDenunciationDeserializer`
    pub const fn new(thread_count: u8) -> Self {
        Self {
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            hash_deserializer: HashDeserializer::new(),
            pubkey_deserializer: PublicKeyDeserializer::new(),
            signature_deserializer: SignatureDeserializer::new(),
        }
    }
}

impl Deserializer<BlockHeaderDenunciation> for BlockHeaderDenunciationDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BlockHeaderDenunciation, E> {
        context(
            "Failed BlockHeader Denunciation deserialization",
            tuple((
                context("Failed public key deserialization", |input| {
                    self.pubkey_deserializer.deserialize(input)
                }),
                context("Failed slot deserialization", |input| {
                    self.slot_deserializer.deserialize(input)
                }),
                context("Failed hash 1 deserialization", |input| {
                    self.hash_deserializer.deserialize(input)
                }),
                context("Failed hash 2 deserialization", |input| {
                    self.hash_deserializer.deserialize(input)
                }),
                context("Failed signature 1 deserialization", |input| {
                    self.signature_deserializer.deserialize(input)
                }),
                context("Failed signature 2 deserialization", |input| {
                    self.signature_deserializer.deserialize(input)
                }),
            )),
        )
        .map(
            |(public_key, slot, hash_1, hash_2, signature_1, signature_2)| {
                BlockHeaderDenunciation {
                    public_key,
                    slot,
                    hash_1,
                    hash_2,
                    signature_1,
                    signature_2,
                }
            },
        )
        .parse(buffer)
    }
}

/// Serializer for `Denunciation`
pub struct DenunciationSerializer {
    endo_de_serializer: EndorsementDenunciationSerializer,
    blkh_de_serializer: BlockHeaderDenunciationSerializer,
    type_id_serializer: U32VarIntSerializer,
}

impl DenunciationSerializer {
    /// Creates a new `BlockHeaderDenunciationSerializer`
    pub const fn new() -> Self {
        Self {
            endo_de_serializer: EndorsementDenunciationSerializer::new(),
            blkh_de_serializer: BlockHeaderDenunciationSerializer::new(),
            type_id_serializer: U32VarIntSerializer::new(),
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
        let de_type_id = DenunciationTypeId::from(value);
        self.type_id_serializer
            .serialize(&u32::from(de_type_id), buffer)?;
        match value {
            Denunciation::Endorsement(de) => {
                self.endo_de_serializer.serialize(de, buffer)?;
            }
            Denunciation::BlockHeader(de) => {
                self.blkh_de_serializer.serialize(de, buffer)?;
            }
        }
        Ok(())
    }
}

const DENUNCIATION_TYPE_ID_VARIANT_COUNT: u32 =
    std::mem::variant_count::<DenunciationTypeId>() as u32;

/// Deserializer for `Denunciation`
pub struct DenunciationDeserializer {
    endo_de_deserializer: EndorsementDenunciationDeserializer,
    blkh_de_deserializer: BlockHeaderDenunciationDeserializer,
    type_id_deserializer: U32VarIntDeserializer,
}

impl DenunciationDeserializer {
    /// Creates a new `DenunciationDeserializer`
    pub const fn new(thread_count: u8, endorsement_count: u32) -> Self {
        Self {
            endo_de_deserializer: EndorsementDenunciationDeserializer::new(
                thread_count,
                endorsement_count,
            ),
            blkh_de_deserializer: BlockHeaderDenunciationDeserializer::new(thread_count),
            type_id_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Excluded(DENUNCIATION_TYPE_ID_VARIANT_COUNT),
            ),
        }
    }
}

impl Deserializer<Denunciation> for DenunciationDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Denunciation, E> {
        let (rem, de_type_id_) = context("Failed Denunciation type id deserialization", |input| {
            self.type_id_deserializer.deserialize(input)
        })
        .parse(buffer)?;

        let de_type_id = DenunciationTypeId::try_from(de_type_id_).map_err(|_| {
            nom::Err::Error(ParseError::from_error_kind(
                buffer,
                nom::error::ErrorKind::Fail,
            ))
        })?;

        match de_type_id {
            DenunciationTypeId::Endorsement => {
                let (rem2, endo_de) = self.endo_de_deserializer.deserialize(rem)?;
                IResult::Ok((rem2, Denunciation::Endorsement(endo_de)))
            }
            DenunciationTypeId::BlockHeader => {
                let (rem2, blkh_de) = self.blkh_de_deserializer.deserialize(rem)?;
                IResult::Ok((rem2, Denunciation::BlockHeader(blkh_de)))
            }
        }
    }
}

// End Ser / Der

// Denunciation Index

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
/// Index for Denunciations in collections (e.g. like a HashMap...)
pub enum DenunciationIndex {
    /// Variant for Block header denunciation index
    BlockHeader {
        /// de slot
        slot: Slot,
    },
    /// Variant for Endorsement denunciation index
    Endorsement {
        /// de slot
        slot: Slot,
        /// de index
        index: u32,
    },
}

impl DenunciationIndex {
    /// Get field: slot
    pub fn get_slot(&self) -> &Slot {
        match self {
            DenunciationIndex::BlockHeader { slot } => slot,
            DenunciationIndex::Endorsement { slot, .. } => slot,
        }
    }

    /// Get field: index (return None for a block header denunciation index)
    pub fn get_index(&self) -> Option<&u32> {
        match self {
            DenunciationIndex::BlockHeader { .. } => None,
            DenunciationIndex::Endorsement { slot: _, index } => Some(index),
        }
    }

    /// Compute the hash
    pub fn get_hash(&self) -> Hash {
        let mut buffer = u32::from(DenunciationIndexTypeId::from(self))
            .to_le_bytes()
            .to_vec();
        match self {
            DenunciationIndex::BlockHeader { slot } => buffer.extend(slot.to_bytes_key()),
            DenunciationIndex::Endorsement { slot, index } => {
                buffer.extend(slot.to_bytes_key());
                buffer.extend(index.to_le_bytes());
            }
        }
        Hash::compute_from(&buffer)
    }
}

/// Create a `DenunciationIndex` from a `Denunciation`
impl From<&Denunciation> for DenunciationIndex {
    fn from(value: &Denunciation) -> Self {
        match value {
            Denunciation::Endorsement(endo_de) => DenunciationIndex::Endorsement {
                slot: endo_de.slot,
                index: endo_de.index,
            },
            Denunciation::BlockHeader(blkh_de) => {
                DenunciationIndex::BlockHeader { slot: blkh_de.slot }
            }
        }
    }
}

/// Create a `DenunciationIndex` from a `DenunciationPrecursor`
impl From<&DenunciationPrecursor> for DenunciationIndex {
    fn from(value: &DenunciationPrecursor) -> Self {
        match value {
            DenunciationPrecursor::Endorsement(de_p) => DenunciationIndex::Endorsement {
                slot: de_p.slot,
                index: de_p.index,
            },
            DenunciationPrecursor::BlockHeader(de_p) => {
                DenunciationIndex::BlockHeader { slot: de_p.slot }
            }
        }
    }
}

impl Ord for DenunciationIndex {
    fn cmp(&self, other: &Self) -> Ordering {
        // key ends with type id so that we avoid prioritizing one type over the other
        // when filling block headers (comparison of tuples is from left to right).
        let self_key = (
            self.get_slot(),
            self.get_index().unwrap_or(&0),
            u32::from(DenunciationIndexTypeId::from(self)),
        );
        let other_key = (
            other.get_slot(),
            other.get_index().unwrap_or(&0),
            u32::from(DenunciationIndexTypeId::from(other)),
        );
        self_key.cmp(&other_key)
    }
}

impl PartialOrd for DenunciationIndex {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// End Denunciation Index

// Denunciation Index ser der

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u32)]
enum DenunciationIndexTypeId {
    BlockHeader = 0,
    Endorsement = 1,
}

impl From<&DenunciationIndex> for DenunciationIndexTypeId {
    fn from(value: &DenunciationIndex) -> Self {
        match value {
            DenunciationIndex::BlockHeader { .. } => DenunciationIndexTypeId::BlockHeader,
            DenunciationIndex::Endorsement { .. } => DenunciationIndexTypeId::Endorsement,
        }
    }
}

/// Serializer for `DenunciationIndex`
pub struct DenunciationIndexSerializer {
    u32_serializer: U32VarIntSerializer,
    slot_serializer: SlotSerializer,
    index_serializer: U32VarIntSerializer,
}

impl DenunciationIndexSerializer {
    /// Creates a new `DenunciationIndexSerializer`
    pub const fn new() -> Self {
        Self {
            u32_serializer: U32VarIntSerializer::new(),
            slot_serializer: SlotSerializer::new(),
            index_serializer: U32VarIntSerializer::new(),
        }
    }
}

impl Default for DenunciationIndexSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<DenunciationIndex> for DenunciationIndexSerializer {
    fn serialize(
        &self,
        value: &DenunciationIndex,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        match value {
            DenunciationIndex::BlockHeader { slot } => {
                self.u32_serializer
                    .serialize(&u32::from(DenunciationIndexTypeId::BlockHeader), buffer)?;
                self.slot_serializer.serialize(slot, buffer)?;
            }
            DenunciationIndex::Endorsement { slot, index } => {
                self.u32_serializer
                    .serialize(&u32::from(DenunciationIndexTypeId::Endorsement), buffer)?;
                self.slot_serializer.serialize(slot, buffer)?;
                self.index_serializer.serialize(index, buffer)?;
            }
        }
        Ok(())
    }
}

/// Deserializer for `DenunciationIndex`
pub struct DenunciationIndexDeserializer {
    id_deserializer: U32VarIntDeserializer,
    slot_deserializer: SlotDeserializer,
    index_deserializer: U32VarIntDeserializer,
}

impl DenunciationIndexDeserializer {
    /// Creates a new `DenunciationIndexDeserializer`
    pub const fn new(thread_count: u8, endorsement_count: u32) -> Self {
        Self {
            id_deserializer: U32VarIntDeserializer::new(Included(0), Included(u32::MAX)),
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            index_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(endorsement_count),
            ),
        }
    }
}

impl Deserializer<DenunciationIndex> for DenunciationIndexDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], DenunciationIndex, E> {
        let (input, id) = self.id_deserializer.deserialize(buffer)?;
        let id = DenunciationIndexTypeId::try_from(id).map_err(|_| {
            nom::Err::Error(ParseError::from_error_kind(
                buffer,
                nom::error::ErrorKind::Eof,
            ))
        })?;

        match id {
            DenunciationIndexTypeId::BlockHeader => {
                context("Failed slot deserialization", |input| {
                    self.slot_deserializer.deserialize(input)
                })
                .map(|slot| DenunciationIndex::BlockHeader { slot })
                .parse(&buffer[1..])
            }
            DenunciationIndexTypeId::Endorsement => context(
                "Failed Endorsement denunciation index",
                tuple((
                    context("Failed slot deserialization", |input| {
                        self.slot_deserializer.deserialize(input)
                    }),
                    context("Failed index deserialization", |input| {
                        self.index_deserializer.deserialize(input)
                    }),
                )),
            )
            .map(|(slot, index)| DenunciationIndex::Endorsement { slot, index })
            .parse(input),
        }
    }
}

// End Denunciation Index ser der

// Denunciation interest

/// DenunciationPrecursor variant for endorsement
#[derive(Debug, Clone)]
pub struct EndorsementDenunciationPrecursor {
    /// secure share endorsement public key
    pub public_key: PublicKey,
    /// endorsement slot
    pub slot: Slot,
    /// endorsement index
    pub index: u32,
    /// secured header partial hash
    hash: Hash,
    /// secured header signature
    signature: Signature,
}

/// DenunciationPrecursor variant for block header
#[derive(Debug, Clone)]
pub struct BlockHeaderDenunciationPrecursor {
    /// secured header public key
    pub public_key: PublicKey,
    /// block header slot
    pub slot: Slot,
    /// secured header partial hash
    hash: Hash,
    /// secured header signature
    signature: Signature,
}

/// Lightweight data for Denunciation creation
/// (avoid storing heavyweight secured header or secure share endorsement, see denunciation pool)
#[derive(Debug, Clone)]
pub enum DenunciationPrecursor {
    /// Endorsement variant
    Endorsement(EndorsementDenunciationPrecursor),
    /// Block header variant
    BlockHeader(BlockHeaderDenunciationPrecursor),
}

impl DenunciationPrecursor {
    /// Get field: slot
    pub fn get_slot(&self) -> &Slot {
        match self {
            DenunciationPrecursor::Endorsement(endo_de_p) => &endo_de_p.slot,
            DenunciationPrecursor::BlockHeader(blkh_de_p) => &blkh_de_p.slot,
        }
    }

    /// Get field: pub key
    pub fn get_public_key(&self) -> &PublicKey {
        match self {
            DenunciationPrecursor::Endorsement(endo_de_p) => &endo_de_p.public_key,
            DenunciationPrecursor::BlockHeader(blkh_de_p) => &blkh_de_p.public_key,
        }
    }
}

impl From<&SecureShareEndorsement> for DenunciationPrecursor {
    fn from(value: &SecureShareEndorsement) -> Self {
        DenunciationPrecursor::Endorsement(EndorsementDenunciationPrecursor {
            public_key: value.content_creator_pub_key,
            slot: value.content.slot,
            index: value.content.index,
            hash: *value.id.get_hash(),
            signature: value.signature,
        })
    }
}

impl From<&SecuredHeader> for DenunciationPrecursor {
    fn from(value: &SecuredHeader) -> Self {
        DenunciationPrecursor::BlockHeader(BlockHeaderDenunciationPrecursor {
            public_key: value.content_creator_pub_key,
            slot: value.content.slot,
            hash: *value.id.get_hash(),
            signature: value.signature,
        })
    }
}

/// Create a new Denunciation from 2 SecureHeader
impl TryFrom<(&DenunciationPrecursor, &DenunciationPrecursor)> for Denunciation {
    type Error = DenunciationError;

    fn try_from(
        (de_i_1, de_i_2): (&DenunciationPrecursor, &DenunciationPrecursor),
    ) -> Result<Self, Self::Error> {
        match (de_i_1, de_i_2) {
            (
                DenunciationPrecursor::BlockHeader(de_i_blkh_1),
                DenunciationPrecursor::BlockHeader(de_i_blkh_2),
            ) => {
                // Cannot use the same block header (here: block header denunciation precursor) twice
                if de_i_blkh_1.slot != de_i_blkh_2.slot
                    || de_i_blkh_1.public_key != de_i_blkh_2.public_key
                    || de_i_blkh_1.hash == de_i_blkh_2.hash
                {
                    return Err(DenunciationError::InvalidInput);
                }

                // Check sig
                let de_i_blkh_1_hash = BlockHeaderDenunciation::compute_hash_for_sig_verif(
                    &de_i_blkh_1.public_key,
                    &de_i_blkh_1.slot,
                    &de_i_blkh_1.hash,
                );
                let de_i_blkh_2_hash = BlockHeaderDenunciation::compute_hash_for_sig_verif(
                    &de_i_blkh_2.public_key,
                    &de_i_blkh_2.slot,
                    &de_i_blkh_2.hash,
                );

                de_i_blkh_1
                    .public_key
                    .verify_signature(&de_i_blkh_1_hash, &de_i_blkh_1.signature)?;
                de_i_blkh_1
                    .public_key
                    .verify_signature(&de_i_blkh_2_hash, &de_i_blkh_2.signature)?;

                Ok(Denunciation::BlockHeader(BlockHeaderDenunciation {
                    public_key: de_i_blkh_1.public_key,
                    slot: de_i_blkh_1.slot,
                    signature_1: de_i_blkh_1.signature,
                    signature_2: de_i_blkh_2.signature,
                    hash_1: de_i_blkh_1.hash,
                    hash_2: de_i_blkh_2.hash,
                }))
            }
            (
                DenunciationPrecursor::Endorsement(de_i_endo_1),
                DenunciationPrecursor::Endorsement(de_i_endo_2),
            ) => {
                // Cannot use the same endorsement (here: endorsement denunciation) twice
                if de_i_endo_1.slot != de_i_endo_2.slot
                    || de_i_endo_1.index != de_i_endo_2.index
                    || de_i_endo_1.public_key != de_i_endo_2.public_key
                    || de_i_endo_1.hash == de_i_endo_2.hash
                {
                    return Err(DenunciationError::InvalidInput);
                }

                // Check sig
                let de_i_endo_1_hash = EndorsementDenunciation::compute_hash_for_sig_verif(
                    &de_i_endo_1.public_key,
                    &de_i_endo_1.slot,
                    &de_i_endo_1.index,
                    &de_i_endo_1.hash,
                );
                let de_i_endo_2_hash = EndorsementDenunciation::compute_hash_for_sig_verif(
                    &de_i_endo_2.public_key,
                    &de_i_endo_2.slot,
                    &de_i_endo_2.index,
                    &de_i_endo_2.hash,
                );

                de_i_endo_1
                    .public_key
                    .verify_signature(&de_i_endo_1_hash, &de_i_endo_1.signature)?;
                de_i_endo_1
                    .public_key
                    .verify_signature(&de_i_endo_2_hash, &de_i_endo_2.signature)?;

                Ok(Denunciation::Endorsement(EndorsementDenunciation {
                    public_key: de_i_endo_1.public_key,
                    slot: de_i_endo_1.slot,
                    index: de_i_endo_1.index,
                    signature_1: de_i_endo_1.signature,
                    signature_2: de_i_endo_2.signature,
                    hash_1: de_i_endo_1.hash,
                    hash_2: de_i_endo_2.hash,
                }))
            }
            _ => {
                // Different enum variants - this is invalid
                Err(DenunciationError::InvalidInput)
            }
        }
    }
}

// End Denunciation interest

// testing

#[cfg(any(test, feature = "testing"))]
impl Denunciation {
    /// Used under testing conditions to validate an instance of Self
    pub fn check_invariants(&self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.is_valid() {
            return Err(format!("Denunciation is invalid: {:?}", self).into());
        }
        Ok(())
    }
}

// end testing

#[cfg(test)]
mod tests {
    use super::*;

    use massa_serialization::DeserializeError;
    use massa_signature::KeyPair;

    use crate::block_id::BlockId;
    use crate::config::{ENDORSEMENT_COUNT, THREAD_COUNT};
    use crate::endorsement::{Endorsement, EndorsementSerializer, SecureShareEndorsement};
    use crate::secure_share::{Id, SecureShareContent};
    use crate::test_exports::{
        gen_block_headers_for_denunciation, gen_endorsements_for_denunciation,
    };

    #[test]
    fn test_endorsement_denunciation() {
        // Create an endorsement denunciation and check if it is valid
        let (_slot, _keypair, s_endorsement_1, s_endorsement_2, _s_endorsement_3) =
            gen_endorsements_for_denunciation(None, None);
        let denunciation: Denunciation = (&s_endorsement_1, &s_endorsement_2).try_into().unwrap();

        assert_eq!(denunciation.is_for_endorsement(), true);
        assert_eq!(denunciation.is_valid(), true);
    }

    #[test]
    fn test_endorsement_denunciation_invalid_1() {
        let (slot, keypair, s_endorsement_1, _s_endorsement_2, _s_endorsement_3) =
            gen_endorsements_for_denunciation(None, None);

        // Try to create a denunciation from 2 endorsements @ != index
        let endorsement_4 = Endorsement {
            slot,
            index: 9,
            endorsed_block: BlockId(Hash::compute_from("foo".as_bytes())),
        };
        let s_endorsement_4 =
            Endorsement::new_verifiable(endorsement_4, EndorsementSerializer::new(), &keypair)
                .unwrap();

        let denunciation = Denunciation::try_from((&s_endorsement_1, &s_endorsement_4));

        assert!(matches!(denunciation, Err(DenunciationError::InvalidInput)));

        // Try to create a denunciation from only 1 endorsement
        let denunciation = Denunciation::try_from((&s_endorsement_1, &s_endorsement_1));

        assert!(matches!(denunciation, Err(DenunciationError::InvalidInput)));
    }

    #[test]
    fn test_endorsement_denunciation_is_for() {
        let (slot, keypair, s_endorsement_1, s_endorsement_2, s_endorsement_3) =
            gen_endorsements_for_denunciation(None, None);

        let denunciation: Denunciation = (&s_endorsement_1, &s_endorsement_2).try_into().unwrap();

        assert_eq!(denunciation.is_for_endorsement(), true);
        assert_eq!(denunciation.is_valid(), true);

        // Try to create a denunciation from 2 endorsements @ != index
        let endorsement_4 = Endorsement {
            slot,
            index: 9,
            endorsed_block: BlockId(Hash::compute_from("foo".as_bytes())),
        };
        let s_endorsement_4 =
            Endorsement::new_verifiable(endorsement_4, EndorsementSerializer::new(), &keypair)
                .unwrap();

        assert_eq!(
            denunciation
                .is_also_for_endorsement(&s_endorsement_4)
                .unwrap(),
            false
        );
        assert_eq!(
            denunciation
                .is_also_for_endorsement(&s_endorsement_3)
                .unwrap(),
            true
        );
        assert_eq!(denunciation.is_valid(), true);
    }

    #[test]
    fn test_block_header_denunciation() {
        // Create an block header denunciation and check if it is valid
        let (_slot, _keypair, s_block_header_1, s_block_header_2, s_block_header_3) =
            gen_block_headers_for_denunciation(None, None);
        let denunciation: Denunciation = (&s_block_header_1, &s_block_header_2).try_into().unwrap();

        assert_eq!(denunciation.is_for_block_header(), true);
        assert_eq!(denunciation.is_valid(), true);
        assert_eq!(
            denunciation
                .is_also_for_block_header(&s_block_header_3)
                .unwrap(),
            true
        );
    }

    #[test]
    fn test_forge_invalid_denunciation() {
        let keypair = KeyPair::generate();
        let slot_1 = Slot::new(4, 2);
        let slot_2 = Slot::new(3, 7);

        let endorsement_1 = Endorsement {
            slot: slot_1,
            index: 0,
            endorsed_block: BlockId(Hash::compute_from("blk1".as_bytes())),
        };

        let s_endorsement_1: SecureShareEndorsement =
            Endorsement::new_verifiable(endorsement_1, EndorsementSerializer::new(), &keypair)
                .unwrap();

        let endorsement_2 = Endorsement {
            slot: slot_2,
            index: 0,
            endorsed_block: BlockId(Hash::compute_from("blk2".as_bytes())),
        };

        let s_endorsement_2: SecureShareEndorsement =
            Endorsement::new_verifiable(endorsement_2, EndorsementSerializer::new(), &keypair)
                .unwrap();

        // from an attacker - building manually a Denunciation object
        let de_forged_1 = Denunciation::Endorsement(EndorsementDenunciation {
            public_key: keypair.get_public_key(),
            slot: slot_1,
            index: 0,
            hash_1: *s_endorsement_1.id.get_hash(), // use only data from s_endorsement_1
            hash_2: *s_endorsement_1.id.get_hash(),
            signature_1: s_endorsement_1.signature,
            signature_2: s_endorsement_1.signature,
        });

        // hash_1 == hash_2 -> this is invalid
        assert_eq!(de_forged_1.is_valid(), false);

        // from an attacker - building manually a Denunciation object
        let de_forged_2 = Denunciation::Endorsement(EndorsementDenunciation {
            public_key: keypair.get_public_key(),
            slot: slot_2,
            index: 0,
            hash_1: *s_endorsement_1.id.get_hash(),
            hash_2: *s_endorsement_2.id.get_hash(),
            signature_1: s_endorsement_1.signature,
            signature_2: s_endorsement_2.signature,
        });

        // An attacker uses an old s_endorsement_1 to forge a Denunciation object @ slot_2
        // This has to be detected if Denunciation are send via the network
        assert_eq!(de_forged_2.is_valid(), false);
    }

    // SER / DER
    #[test]
    fn test_endorsement_denunciation_ser_der() {
        let (_, _, s_endorsement_1, s_endorsement_2, _) =
            gen_endorsements_for_denunciation(None, None);

        let denunciation = Denunciation::try_from((&s_endorsement_1, &s_endorsement_2)).unwrap();

        let mut buffer = Vec::new();
        let de_ser = EndorsementDenunciationSerializer::new();

        match denunciation {
            Denunciation::Endorsement(de) => {
                de_ser.serialize(&de, &mut buffer).unwrap();
                let de_der =
                    EndorsementDenunciationDeserializer::new(THREAD_COUNT, ENDORSEMENT_COUNT);

                let (rem, de_der_res) = de_der.deserialize::<DeserializeError>(&buffer).unwrap();

                assert_eq!(rem.is_empty(), true);
                assert_eq!(de, de_der_res);
            }
            Denunciation::BlockHeader(_) => {
                unimplemented!()
            }
        }
    }

    #[test]
    fn test_block_header_denunciation_ser_der() {
        let (_, _, s_block_header_1, s_block_header_2, _) =
            gen_block_headers_for_denunciation(None, None);
        let denunciation: Denunciation = (&s_block_header_1, &s_block_header_2).try_into().unwrap();

        let mut buffer = Vec::new();
        let de_ser = BlockHeaderDenunciationSerializer::new();

        match denunciation {
            Denunciation::Endorsement(_) => {
                unimplemented!()
            }
            Denunciation::BlockHeader(de) => {
                de_ser.serialize(&de, &mut buffer).unwrap();
                let de_der = BlockHeaderDenunciationDeserializer::new(THREAD_COUNT);

                let (rem, de_der_res) = de_der.deserialize::<DeserializeError>(&buffer).unwrap();

                assert_eq!(rem.is_empty(), true);
                assert_eq!(de, de_der_res);
            }
        }
    }

    #[test]
    fn test_denunciation_ser_der() {
        let (_, _, s_block_header_1, s_block_header_2, _) =
            gen_block_headers_for_denunciation(None, None);
        let denunciation: Denunciation = (&s_block_header_1, &s_block_header_2).try_into().unwrap();

        let mut buffer = Vec::new();
        let de_ser = DenunciationSerializer::new();

        de_ser.serialize(&denunciation, &mut buffer).unwrap();
        let de_der = DenunciationDeserializer::new(THREAD_COUNT, ENDORSEMENT_COUNT);

        let (rem, de_der_res) = de_der.deserialize::<DeserializeError>(&buffer).unwrap();

        assert_eq!(rem.is_empty(), true);
        assert_eq!(denunciation, de_der_res);

        let (_, _, s_endorsement_1, s_endorsement_2, _) =
            gen_endorsements_for_denunciation(None, None);
        let denunciation = Denunciation::try_from((&s_endorsement_1, &s_endorsement_2)).unwrap();
        buffer.clear();

        de_ser.serialize(&denunciation, &mut buffer).unwrap();
        let (rem, de_der_res) = de_der.deserialize::<DeserializeError>(&buffer).unwrap();
        assert_eq!(rem.is_empty(), true);
        assert_eq!(denunciation, de_der_res);
    }

    #[test]
    fn test_denunciation_precursor() {
        let (_, _, s_block_header_1, s_block_header_2, _) =
            gen_block_headers_for_denunciation(None, None);
        let denunciation: Denunciation = (&s_block_header_1, &s_block_header_2).try_into().unwrap();

        let de_p_1 = DenunciationPrecursor::from(&s_block_header_1);
        let de_p_2 = DenunciationPrecursor::from(&s_block_header_2);
        let denunciation_2: Denunciation = (&de_p_1, &de_p_2).try_into().unwrap();

        assert_eq!(denunciation, denunciation_2);

        let (_, _, s_endorsement_1, s_endorsement_2, _) =
            gen_endorsements_for_denunciation(None, None);
        let denunciation_3 = Denunciation::try_from((&s_endorsement_1, &s_endorsement_2)).unwrap();

        let de_p_3 = DenunciationPrecursor::from(&s_endorsement_1);
        let de_p_4 = DenunciationPrecursor::from(&s_endorsement_2);
        let denunciation_4: Denunciation = (&de_p_3, &de_p_4).try_into().unwrap();

        assert_eq!(denunciation_3, denunciation_4);
    }

    #[test]
    fn test_denunciation_index_ser_der() {
        let (_, _, s_block_header_1, s_block_header_2, _) =
            gen_block_headers_for_denunciation(None, None);
        let denunciation_1: Denunciation =
            (&s_block_header_1, &s_block_header_2).try_into().unwrap();
        let denunciation_index_1 = DenunciationIndex::from(&denunciation_1);

        let (_, _, s_endorsement_1, s_endorsement_2, _) =
            gen_endorsements_for_denunciation(None, None);
        let denunciation_2 = Denunciation::try_from((&s_endorsement_1, &s_endorsement_2)).unwrap();
        let denunciation_index_2 = DenunciationIndex::from(&denunciation_2);

        let mut buffer = Vec::new();
        let de_idx_ser = DenunciationIndexSerializer::new();
        de_idx_ser
            .serialize(&denunciation_index_1, &mut buffer)
            .unwrap();
        let de_idx_der = DenunciationIndexDeserializer::new(THREAD_COUNT, ENDORSEMENT_COUNT);
        let (rem, de_idx_der_res) = de_idx_der.deserialize::<DeserializeError>(&buffer).unwrap();

        assert!(rem.is_empty());
        assert_eq!(denunciation_index_1, de_idx_der_res);

        let mut buffer = Vec::new();
        let de_idx_ser = DenunciationIndexSerializer::new();
        de_idx_ser
            .serialize(&denunciation_index_2, &mut buffer)
            .unwrap();
        let de_idx_der = DenunciationIndexDeserializer::new(THREAD_COUNT, ENDORSEMENT_COUNT);
        let (rem, de_idx_der_res) = de_idx_der.deserialize::<DeserializeError>(&buffer).unwrap();

        assert!(rem.is_empty());
        assert_eq!(denunciation_index_2, de_idx_der_res);
    }
}
