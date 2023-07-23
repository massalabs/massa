use crate::block_id::{BlockId, BlockIdDeserializer, BlockIdSerializer};
use crate::denunciation::{Denunciation, DenunciationDeserializer, DenunciationSerializer};
use crate::endorsement::{
    Endorsement, EndorsementDeserializerLW, EndorsementId, EndorsementSerializer,
    EndorsementSerializerLW, SecureShareEndorsement,
};
use crate::secure_share::{
    SecureShare, SecureShareContent, SecureShareDeserializer, SecureShareSerializer,
};
use crate::slot::{Slot, SlotDeserializer, SlotSerializer};
use massa_hash::{Hash, HashDeserializer};
use massa_serialization::{
    Deserializer, OptionDeserializer, OptionSerializer, SerializeError, Serializer,
    U32VarIntDeserializer, U32VarIntSerializer,
};
use massa_signature::PublicKey;
use nom::branch::alt;
use nom::bytes::complete::tag;
use nom::error::{context, ContextError, ParseError};
use nom::multi::{count, length_count};
use nom::sequence::{preceded, tuple};
use nom::{IResult, Parser};
use serde::{Deserialize, Serialize};
use std::collections::Bound::{Excluded, Included};
use std::collections::HashSet;
use std::fmt::Formatter;

/// block header
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    /// current network version
    pub current_version: u32,
    /// announced network version
    pub announced_version: Option<u32>,
    /// slot
    pub slot: Slot,
    /// parents
    pub parents: Vec<BlockId>,
    /// all operations hash
    pub operation_merkle_root: Hash,
    /// endorsements
    pub endorsements: Vec<SecureShareEndorsement>,
    /// denunciations
    pub denunciations: Vec<Denunciation>,
}

// TODO: gh-issue #3398
#[cfg(any(test, feature = "testing"))]
impl BlockHeader {
    /// This is an intentional duplication of invariant checks. In production code,
    /// these checks are dispersed throughout the deserialization process. This test-only function
    /// allows all the checks to be in one place.
    fn assert_invariants(
        &self,
        thread_count: u8,
        endorsement_count: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.slot.period == 0 {
            if !self.parents.is_empty() {
                return Err("Invariant broken: genesis block with parent(s)".into());
            }
            if !self.endorsements.is_empty() {
                return Err("Invariant broken: genesis block with endorsement(s)".into());
            }
        } else {
            if self.parents.len() != thread_count as usize {
                return Err(
                    "Invariant broken: non-genesis block with incorrect number of parents".into(),
                );
            }
            if self.endorsements.len() > endorsement_count as usize {
                return Err("Invariant broken: endorsement count too high".into());
            }

            let parent_id = self.parents[self.slot.thread as usize];
            for endo in self.endorsements.iter() {
                if endo.content.endorsed_block != parent_id {
                    return Err("Invariant broken: endorsement doesn't match parent".into());
                }
            }
        }

        // assert that the endorsement indexes are all unique...
        let mut set = HashSet::new();
        for endo in self.endorsements.iter() {
            // ...and check signatures + invariants while at it
            endo.check_invariants()?;

            if !set.insert(endo.content.index) {
                return Err("Endorsement duplicate index found".into());
            }
        }

        for de in self.denunciations.iter() {
            de.check_invariants()?;
        }

        Ok(())
    }
}

/// BlockHeader wrapped up alongside verification data
pub type SecuredHeader = SecureShare<BlockHeader, BlockId>;

impl SecureShareContent for BlockHeader {
    /// compute the signed hash
    fn compute_signed_hash(&self, public_key: &PublicKey, content_hash: &Hash) -> Hash {
        let mut signed_data: Vec<u8> = Vec::new();
        signed_data.extend(public_key.to_bytes());
        signed_data.extend(BlockHeaderDenunciationData::new(self.slot).to_bytes());
        signed_data.extend(content_hash.to_bytes());
        Hash::compute_from(&signed_data)
    }
}

impl SecuredHeader {
    /// gets the header fitness
    pub fn get_fitness(&self) -> u64 {
        (self.content.endorsements.len() as u64) + 1
    }
    // TODO: gh-issue #3398
    #[allow(dead_code)]
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn assert_invariants(
        &self,
        thread_count: u8,
        endorsement_count: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.content
            .assert_invariants(thread_count, endorsement_count)?;
        self.verify_signature()
            .map_err(|er| format!("{}", er).into())
    }
}

/// Serializer for `BlockHeader`
pub struct BlockHeaderSerializer {
    slot_serializer: SlotSerializer,
    endorsement_serializer: SecureShareSerializer,
    endorsement_content_serializer: EndorsementSerializerLW,
    denunciation_serializer: DenunciationSerializer,
    u32_serializer: U32VarIntSerializer,
    opt_serializer: OptionSerializer<u32, U32VarIntSerializer>,
    block_id_serializer: BlockIdSerializer,
}

impl BlockHeaderSerializer {
    /// Creates a new `BlockHeaderSerializer`
    pub fn new() -> Self {
        Self {
            slot_serializer: SlotSerializer::new(),
            endorsement_serializer: SecureShareSerializer::new(),
            u32_serializer: U32VarIntSerializer::new(),
            opt_serializer: OptionSerializer::new(U32VarIntSerializer),
            endorsement_content_serializer: EndorsementSerializerLW::new(),
            denunciation_serializer: DenunciationSerializer::new(),
            block_id_serializer: BlockIdSerializer::new(),
        }
    }
}

impl Default for BlockHeaderSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<BlockHeader> for BlockHeaderSerializer {
    /// ## Example:
    /// ```rust
    /// use massa_models::{block_id::BlockId, block_header::BlockHeader, block_header::BlockHeaderSerializer};
    /// use massa_models::endorsement::{Endorsement, EndorsementSerializer};
    /// use massa_models::secure_share::SecureShareContent;
    /// use massa_models::{config::THREAD_COUNT, slot::Slot};
    /// use massa_hash::Hash;
    /// use massa_signature::KeyPair;
    /// use massa_serialization::Serializer;
    ///
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let parents = (0..THREAD_COUNT)
    ///   .map(|i| BlockId::generate_from_hash(Hash::compute_from(&[i])))
    ///   .collect();
    /// let header = BlockHeader {
    ///   current_version: 0,
    ///   announced_version: None,
    ///   slot: Slot::new(1, 1),
    ///   parents,
    ///   operation_merkle_root: Hash::compute_from("mno".as_bytes()),
    ///   endorsements: vec![
    ///     Endorsement::new_verifiable(
    ///        Endorsement {
    ///          slot: Slot::new(1, 1),
    ///          index: 1,
    ///          endorsed_block: BlockId::generate_from_hash(Hash::compute_from("blk1".as_bytes())),
    ///        },
    ///     EndorsementSerializer::new(),
    ///     &keypair,
    ///     )
    ///     .unwrap(),
    ///     Endorsement::new_verifiable(
    ///       Endorsement {
    ///         slot: Slot::new(4, 0),
    ///         index: 3,
    ///         endorsed_block: BlockId::generate_from_hash(Hash::compute_from("blk2".as_bytes())),
    ///       },
    ///     EndorsementSerializer::new(),
    ///     &keypair,
    ///     )
    ///     .unwrap(),
    ///    ],
    ///   denunciations: vec![],
    /// };
    /// let mut buffer = vec![];
    /// BlockHeaderSerializer::new().serialize(&header, &mut buffer).unwrap();
    /// ```
    fn serialize(&self, value: &BlockHeader, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        // network versions
        self.u32_serializer
            .serialize(&value.current_version, buffer)?;
        self.opt_serializer
            .serialize(&value.announced_version, buffer)?;

        // slot
        self.slot_serializer.serialize(&value.slot, buffer)?;

        // parents (note: there should be none if slot period=0)
        if value.parents.is_empty() {
            buffer.push(0);
        } else {
            buffer.push(1);
        }
        for parent_h in value.parents.iter() {
            self.block_id_serializer.serialize(parent_h, buffer)?;
        }

        // operations merkle root
        buffer.extend(value.operation_merkle_root.to_bytes());

        self.u32_serializer.serialize(
            &value.endorsements.len().try_into().map_err(|err| {
                SerializeError::GeneralError(format!("too many endorsements: {}", err))
            })?,
            buffer,
        )?;

        for endorsement in value.endorsements.iter() {
            self.endorsement_serializer.serialize_with(
                &self.endorsement_content_serializer,
                endorsement,
                buffer,
            )?;
        }
        self.u32_serializer.serialize(
            &value.denunciations.len().try_into().map_err(|err| {
                SerializeError::GeneralError(format!("too many denunciations: {}", err))
            })?,
            buffer,
        )?;
        for denunciation in value.denunciations.iter() {
            self.denunciation_serializer
                .serialize(denunciation, buffer)?;
        }

        Ok(())
    }
}

/// Deserializer for `BlockHeader`
pub struct BlockHeaderDeserializer {
    slot_deserializer: SlotDeserializer,
    endorsement_serializer: EndorsementSerializer,
    endorsement_len_deserializer: U32VarIntDeserializer,
    hash_deserializer: HashDeserializer,
    thread_count: u8,
    endorsement_count: u32,
    last_start_period: Option<u64>,
    denunciation_len_deserializer: U32VarIntDeserializer,
    denunciation_deserializer: DenunciationDeserializer,
    network_versions_deserializer: U32VarIntDeserializer,
    opt_deserializer: OptionDeserializer<u32, U32VarIntDeserializer>,
    block_id_deserializer: BlockIdDeserializer,
}

impl BlockHeaderDeserializer {
    /// Creates a new `BlockHeaderDeserializer`
    /// If last_start_period is Some(lsp), then the deserializer will check for valid (non)-genesis blocks
    pub fn new(
        thread_count: u8,
        endorsement_count: u32,
        max_denunciations_in_block_header: u32,
        last_start_period: Option<u64>,
    ) -> Self {
        Self {
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            endorsement_serializer: EndorsementSerializer::new(),
            endorsement_len_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(endorsement_count),
            ),
            hash_deserializer: HashDeserializer::new(),
            denunciation_len_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(max_denunciations_in_block_header),
            ),
            network_versions_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(u32::MAX),
            ),
            opt_deserializer: OptionDeserializer::new(U32VarIntDeserializer::new(
                Included(0),
                Included(u32::MAX),
            )),
            denunciation_deserializer: DenunciationDeserializer::new(
                thread_count,
                endorsement_count,
            ),
            block_id_deserializer: BlockIdDeserializer::new(),
            thread_count,
            endorsement_count,
            last_start_period,
        }
    }
}

impl Deserializer<BlockHeader> for BlockHeaderDeserializer {
    /// ## Example:
    /// ```rust
    /// use massa_models::block_header::{BlockHeader, BlockHeaderDeserializer, BlockHeaderSerializer};
    /// use massa_models::block_id::{BlockId};
    /// use massa_models::{config::THREAD_COUNT, slot::Slot, secure_share::SecureShareContent};
    /// use massa_models::endorsement::{Endorsement, EndorsementSerializer};
    /// use massa_hash::Hash;
    /// use massa_signature::KeyPair;
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    ///
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let parents: Vec<BlockId> = (0..THREAD_COUNT)
    ///   .map(|i| BlockId::generate_from_hash(Hash::compute_from(&[i])))
    ///   .collect();
    /// let header = BlockHeader {
    ///   current_version: 0,
    ///   announced_version: None,
    ///   slot: Slot::new(1, 1),
    ///   parents: parents.clone(),
    ///   operation_merkle_root: Hash::compute_from("mno".as_bytes()),
    ///   endorsements: vec![
    ///     Endorsement::new_verifiable(
    ///        Endorsement {
    ///          slot: Slot::new(1, 1),
    ///          index: 0,
    ///          endorsed_block: parents[1].clone(),
    ///        },
    ///     EndorsementSerializer::new(),
    ///     &keypair,
    ///     )
    ///     .unwrap(),
    ///     Endorsement::new_verifiable(
    ///       Endorsement {
    ///         slot: Slot::new(1, 1),
    ///         index: 1,
    ///         endorsed_block: parents[1].clone(),
    ///       },
    ///     EndorsementSerializer::new(),
    ///     &keypair,
    ///     )
    ///     .unwrap(),
    ///    ],
    ///    denunciations: vec![],
    /// };
    /// let mut buffer = vec![];
    /// BlockHeaderSerializer::new().serialize(&header, &mut buffer).unwrap();
    /// let (rest, deserialized_header) = BlockHeaderDeserializer::new(32, 9, 10, Some(0)).deserialize::<DeserializeError>(&buffer).unwrap();
    /// assert_eq!(rest.len(), 0);
    /// let mut buffer2 = Vec::new();
    /// BlockHeaderSerializer::new().serialize(&deserialized_header, &mut buffer2).unwrap();
    /// assert_eq!(buffer, buffer2);
    /// ```
    #[allow(clippy::type_complexity)]
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BlockHeader, E> {
        let (rest, (current_version, announced_version, slot, parents, operation_merkle_root)): (
            &[u8],
            (u32, Option<u32>, Slot, Vec<BlockId>, Hash),
        ) = context("Failed BlockHeader deserialization", |input| {
            let (rest, (current_version, announced_version, slot, parents)) = tuple((
                context("Failed current_version deserialization", |input| {
                    self.network_versions_deserializer.deserialize(input)
                }),
                context("Failed announced_version deserialization", |input| {
                    self.opt_deserializer.deserialize(input)
                }),
                context("Failed slot deserialization", |input| {
                    self.slot_deserializer.deserialize(input)
                }),
                context(
                    "Failed parents deserialization",
                    alt((
                        preceded(tag(&[0]), |input| Ok((input, Vec::new()))),
                        preceded(
                            tag(&[1]),
                            count(
                                context("Failed block_id deserialization", |input| {
                                    self.block_id_deserializer.deserialize(input)
                                }),
                                self.thread_count as usize,
                            ),
                        ),
                    )),
                ),
            ))
            .parse(input)?;

            // validate the parent/slot invariants before moving on to other fields
            if let Some(last_start_period) = self.last_start_period {
                if slot.period == last_start_period && !parents.is_empty() {
                    return Err(nom::Err::Failure(ContextError::add_context(
                        rest,
                        "Genesis block cannot contain parents",
                        ParseError::from_error_kind(rest, nom::error::ErrorKind::Fail),
                    )));
                } else if slot.period != last_start_period
                    && parents.len() != self.thread_count as usize
                {
                    return Err(nom::Err::Failure(ContextError::add_context(
                        rest,
                        "Non-genesis block must have same numbers of parents as threads count",
                        ParseError::from_error_kind(rest, nom::error::ErrorKind::Fail),
                    )));
                }
            }

            let (rest, merkle) = context("Failed operation_merkle_root", |input| {
                self.hash_deserializer.deserialize(input)
            })
            .parse(rest)?;
            Ok((
                rest,
                (current_version, announced_version, slot, parents, merkle),
            ))
        })
        .parse(buffer)?;

        if parents.is_empty() {
            let res = BlockHeader {
                current_version,
                announced_version,
                slot,
                parents,
                operation_merkle_root,
                endorsements: Vec::new(),
                denunciations: Vec::new(),
            };

            // TODO: gh-issue #3398
            #[cfg(any(test, feature = "testing"))]
            res.assert_invariants(self.thread_count, self.endorsement_count)
                .unwrap();

            return Ok((
                &rest[2..], // Because there is 0 endorsements & 0 denunciations, we have a remaining [0, 0] in rest and we don't need it
                res,
            ));
        }

        // Now deser the endorsements (which were light-weight serialized)
        let endorsement_deserializer =
            SecureShareDeserializer::new(EndorsementDeserializerLW::new(
                self.endorsement_count,
                slot,
                parents[slot.thread as usize],
            ));

        let parent_id = parents[slot.thread as usize];
        let (rest, endorsements): (&[u8], Vec<SecureShare<Endorsement, EndorsementId>>) = context(
            "Failed endorsements deserialization",
            length_count::<&[u8], SecureShare<Endorsement, EndorsementId>, u32, E, _, _>(
                context("Failed length deserialization", |input| {
                    self.endorsement_len_deserializer.deserialize(input)
                }),
                context("Failed endorsement deserialization", |input| {
                    let (rest, endo) = endorsement_deserializer
                        .deserialize_with(&self.endorsement_serializer, input)?;

                    if endo.content.endorsed_block != parent_id {
                        return Err(nom::Err::Failure(ContextError::add_context(
                            rest,
                            "Endorsement does not match block parents",
                            ParseError::from_error_kind(rest, nom::error::ErrorKind::Fail),
                        )));
                    }

                    Ok((rest, endo))
                }),
            ),
        )
        .parse(rest)?;

        let mut set = HashSet::new();
        for end in endorsements.iter() {
            if !set.insert(end.content.index) {
                return Err(nom::Err::Failure(ContextError::add_context(
                    rest,
                    "Duplicate endorsement index found",
                    ParseError::from_error_kind(rest, nom::error::ErrorKind::Fail),
                )));
            }
        }

        let (rest, denunciations): (&[u8], Vec<Denunciation>) = context(
            "Failed denunciations deserialization",
            length_count::<&[u8], Denunciation, u32, E, _, _>(
                context("Failed length deserialization", |input| {
                    let (res, count) = self.denunciation_len_deserializer.deserialize(input)?;
                    IResult::Ok((res, count))
                }),
                context("Failed denunciation deserialization", |input| {
                    self.denunciation_deserializer.deserialize(input)
                }),
            ),
        )
        .parse(rest)?;

        let header = BlockHeader {
            current_version,
            announced_version,
            slot,
            parents,
            operation_merkle_root,
            endorsements,
            denunciations,
        };

        // TODO: gh-issue #3398
        #[cfg(any(test, feature = "testing"))]
        header
            .assert_invariants(self.thread_count, self.endorsement_count)
            .unwrap();

        Ok((rest, header))
    }
}

impl std::fmt::Display for BlockHeader {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "\t(period: {}, thread: {})",
            self.slot.period, self.slot.thread,
        )?;
        writeln!(f, "\tMerkle root: {}", self.operation_merkle_root,)?;
        writeln!(f, "\tParents: ")?;
        for id in self.parents.iter() {
            let str_id = id.to_string();
            writeln!(f, "\t\t{}", str_id)?;
        }
        if self.parents.is_empty() {
            writeln!(f, "No parents found: This is a genesis header")?;
        }
        writeln!(f, "\tEndorsements:")?;
        for ed in self.endorsements.iter() {
            writeln!(f, "\t\t-----")?;
            writeln!(f, "\t\tId: {}", ed.id)?;
            writeln!(f, "\t\tIndex: {}", ed.content.index)?;
            writeln!(f, "\t\tEndorsed slot: {}", ed.content.slot)?;
            writeln!(
                f,
                "\t\tEndorser's public key: {}",
                ed.content_creator_pub_key
            )?;
            writeln!(f, "\t\tEndorsed block: {}", ed.content.endorsed_block)?;
            writeln!(f, "\t\tSignature: {}", ed.signature)?;
        }
        if self.endorsements.is_empty() {
            writeln!(f, "\tNo endorsements found")?;
        }
        Ok(())
    }
}

/// A denunciation data for block header
#[derive(Debug)]
pub struct BlockHeaderDenunciationData {
    slot: Slot,
}

impl BlockHeaderDenunciationData {
    /// Create a new DenunciationData for block hedader
    pub fn new(slot: Slot) -> Self {
        Self { slot }
    }

    /// Get byte array
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend(self.slot.to_bytes_key());
        buf
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use massa_serialization::DeserializeError;

    use crate::config::{ENDORSEMENT_COUNT, MAX_DENUNCIATIONS_PER_BLOCK_HEADER, THREAD_COUNT};

    use crate::test_exports::{
        gen_block_headers_for_denunciation, gen_endorsements_for_denunciation,
    };
    use massa_signature::{verify_signature_batch, KeyPair};

    // Only for testing purpose
    impl PartialEq for BlockHeader {
        fn eq(&self, other: &Self) -> bool {
            self.slot == other.slot
                && self.parents == other.parents
                && self.operation_merkle_root == other.operation_merkle_root
                && self.endorsements == other.endorsements
                && self.denunciations == other.denunciations
        }
    }

    #[test]
    fn test_block_header_ser_der() {
        let keypair = KeyPair::generate(0).unwrap();

        let slot = Slot::new(7, 1);
        let parents_1: Vec<BlockId> = (0..THREAD_COUNT)
            .map(|i| BlockId::generate_from_hash(Hash::compute_from(&[i])))
            .collect();

        let endorsement_1 = Endorsement {
            slot: slot.clone(),
            index: 1,
            endorsed_block: parents_1[1].clone(),
        };

        assert_eq!(parents_1[1], endorsement_1.endorsed_block);

        let s_endorsement_1: SecureShareEndorsement =
            Endorsement::new_verifiable(endorsement_1, EndorsementSerializer::new(), &keypair)
                .unwrap();

        let (slot_a, _, s_header_1, s_header_2, _) = gen_block_headers_for_denunciation(None, None);
        assert!(slot_a < slot);
        let de_a = Denunciation::try_from((&s_header_1, &s_header_2)).unwrap();
        let (slot_b, _, s_endo_1, s_endo_2, _) = gen_endorsements_for_denunciation(None, None);
        assert!(slot_b < slot);
        let de_b = Denunciation::try_from((&s_endo_1, &s_endo_2)).unwrap();

        let block_header_1 = BlockHeader {
            current_version: 0,
            announced_version: None,
            slot,
            parents: parents_1,
            operation_merkle_root: Hash::compute_from("mno".as_bytes()),
            endorsements: vec![s_endorsement_1.clone()],
            denunciations: vec![de_a.clone(), de_b.clone()],
        };

        let mut buffer = Vec::new();
        let ser = BlockHeaderSerializer::new();
        ser.serialize(&block_header_1, &mut buffer).unwrap();
        let der = BlockHeaderDeserializer::new(
            THREAD_COUNT,
            ENDORSEMENT_COUNT,
            MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            None,
        );

        let (rem, block_header_der) = der.deserialize::<DeserializeError>(&buffer).unwrap();

        assert_eq!(rem.is_empty(), true);
        assert_eq!(block_header_1, block_header_der);
    }

    #[test]
    fn test_verify_sig_batch() {
        let (_slot, _keypair, secured_header_1, secured_header_2, secured_header_3) =
            gen_block_headers_for_denunciation(None, None);

        // Test with batch len == 1 (no // verif)
        let batch_1 = [(
            secured_header_1.compute_signed_hash(),
            secured_header_1.signature,
            secured_header_1.content_creator_pub_key,
        )];
        verify_signature_batch(&batch_1).unwrap();

        // Test with batch len > 1 (// verif)
        let batch_2 = [
            (
                secured_header_1.compute_signed_hash(),
                secured_header_1.signature,
                secured_header_1.content_creator_pub_key,
            ),
            (
                secured_header_2.compute_signed_hash(),
                secured_header_2.signature,
                secured_header_2.content_creator_pub_key,
            ),
            (
                secured_header_3.compute_signed_hash(),
                secured_header_3.signature,
                secured_header_3.content_creator_pub_key,
            ),
        ];
        verify_signature_batch(&batch_2).unwrap();
    }
}
