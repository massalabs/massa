use crate::{block_status::StorageOrBlock, error::ConsensusError};
use massa_models::{
    active_block::ActiveBlock,
    block::{Block, BlockDeserializer, BlockDeserializerArgs, SecureShareBlock},
    block_id::{BlockId, BlockIdDeserializer, BlockIdSerializer},
    prehash::PreHashMap,
    secure_share::{SecureShareDeserializer, SecureShareSerializer},
};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::branch::alt;
use nom::{
    bytes::complete::tag,
    combinator::value,
    error::{ContextError, ParseError},
    multi::count,
    sequence::{preceded, tuple},
};
use nom::{error::context, IResult, Parser};
use serde::{Deserialize, Serialize};
use std::ops::Bound::Included;

/// Exportable version of `ActiveBlock`
/// Fields that can be easily recomputed were left out
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportActiveBlock {
    /// The block.
    pub block: SecureShareBlock,
    /// one `(block id, period)` per thread ( if not genesis )
    pub parents: Vec<(BlockId, u64)>,
    /// for example has its fitness reached the given threshold
    pub is_final: bool,
}

impl ExportActiveBlock {
    /// conversion from active block to export active block
    pub fn from_active_block(a_block: &ActiveBlock, storage_or_block: &StorageOrBlock) -> Self {
        // TODO: if we decide that endorsements are separate, also gather endorsements here
        ExportActiveBlock {
            parents: a_block.parents.clone(),
            is_final: a_block.is_final,
            block: storage_or_block.clone_block(&a_block.block_id),
        }
    }

    /// Parent block IDs must match the signed header (order and content).
    /// Periods are not in the header and remain bootstrap metadata.
    fn check_parents_match_header(&self, thread_count: u8) -> Result<(), String> {
        let header_parents = &self.block.content.header.content.parents;
        if self.parents.is_empty() {
            if !header_parents.is_empty() {
                return Err(format!(
                    "ExportActiveBlock {} has empty parents but header lists {} parents",
                    self.block.id,
                    header_parents.len()
                ));
            }
            return Ok(());
        }
        let expected_len = thread_count as usize;
        if self.parents.len() != expected_len || header_parents.len() != expected_len {
            return Err(format!(
                "ExportActiveBlock {} parents length mismatch: export={}, header={}, expected={}",
                self.block.id,
                self.parents.len(),
                header_parents.len(),
                expected_len
            ));
        }
        for (thread, ((export_parent, _), header_parent)) in
            self.parents.iter().zip(header_parents.iter()).enumerate()
        {
            if export_parent != header_parent {
                return Err(format!(
                    "ExportActiveBlock {} parent id mismatch in thread {}: export={} header={}",
                    self.block.id, thread, export_parent, header_parent
                ));
            }
        }
        Ok(())
    }

    /// consuming conversion from `ExportActiveBlock` to `ActiveBlock`
    pub fn to_active_block(
        self,
        thread_count: u8,
    ) -> Result<(ActiveBlock, StorageOrBlock), ConsensusError> {
        // Also enforced in ExportActiveBlockDeserializer for the bootstrap wire
        // path; kept here for ExportActiveBlock values that did not come from it
        // (e.g. tests or other construction).
        self.check_parents_match_header(thread_count)
            .map_err(ConsensusError::ContainerInconsistency)?;

        // create ActiveBlock
        let active_block = ActiveBlock {
            creator_address: self.block.content_creator_address,
            block_id: self.block.id,
            parents: self.parents.clone(),
            children: vec![PreHashMap::default(); thread_count as usize], // will be computed once the full graph is available
            descendants: Default::default(), // will be computed once the full graph is available
            is_final: self.is_final,
            slot: self.block.content.header.content.slot,
            fitness: self.block.get_fitness(),
            same_thread_parent_creator: None, // will be computed once the full graph is available
        };

        Ok((active_block, StorageOrBlock::Block(Box::new(self.block))))
    }
}

/// Basic serializer of `ExportActiveBlock`
#[derive(Default)]
pub struct ExportActiveBlockSerializer {
    sec_share_serializer: SecureShareSerializer,
    period_serializer: U64VarIntSerializer,
    block_id_serializer: BlockIdSerializer,
}

impl ExportActiveBlockSerializer {
    /// Create a new `ExportActiveBlockSerializer`
    pub fn new() -> Self {
        ExportActiveBlockSerializer {
            sec_share_serializer: SecureShareSerializer::new(),
            period_serializer: U64VarIntSerializer::new(),
            block_id_serializer: BlockIdSerializer::new(),
        }
    }
}

impl Serializer<ExportActiveBlock> for ExportActiveBlockSerializer {
    fn serialize(
        &self,
        value: &ExportActiveBlock,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        // block
        self.sec_share_serializer.serialize(&value.block, buffer)?;

        // parents with periods
        // note: there should be no parents for genesis blocks
        buffer.push(u8::from(!value.parents.is_empty()));
        for (hash, period) in value.parents.iter() {
            self.block_id_serializer.serialize(hash, buffer)?;
            self.period_serializer.serialize(period, buffer)?;
        }

        // finality
        buffer.push(u8::from(value.is_final));

        Ok(())
    }
}

/// Basic deserializer of `ExportActiveBlock`
pub struct ExportActiveBlockDeserializer {
    sec_share_block_deserializer: SecureShareDeserializer<Block, BlockDeserializer>,
    block_id_deserializer: BlockIdDeserializer,
    period_deserializer: U64VarIntDeserializer,
    thread_count: u8,
}

impl ExportActiveBlockDeserializer {
    /// Create a new `ExportActiveBlockDeserializer`
    // TODO: check if we can remove this?
    #[allow(clippy::too_many_arguments)]
    pub fn new(block_der_args: BlockDeserializerArgs) -> Self {
        let thread_count = block_der_args.thread_count;
        let chain_id = block_der_args.chain_id;
        ExportActiveBlockDeserializer {
            sec_share_block_deserializer: SecureShareDeserializer::new(
                BlockDeserializer::new(block_der_args),
                chain_id,
            ),
            block_id_deserializer: BlockIdDeserializer::new(),
            period_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
            thread_count,
        }
    }
}

impl Deserializer<ExportActiveBlock> for ExportActiveBlockDeserializer {
    /// ## Example:
    /// ```rust
    /// use massa_consensus_exports::export_active_block::{ExportActiveBlock, ExportActiveBlockDeserializer, ExportActiveBlockSerializer};
    /// use massa_models::{ledger::LedgerChanges, config::THREAD_COUNT, rolls::RollUpdates, block::{Block, BlockSerializer}, prehash::PreHashSet, endorsement::{Endorsement, EndorsementSerializer}, slot::Slot, secure_share::SecureShareContent};
    /// use massa_models::block_id::BlockId;
    /// use massa_models::block_header::{BlockHeader, BlockHeaderSerializer};
    /// use massa_hash::Hash;
    /// use std::collections::HashSet;
    /// use massa_models::block::BlockDeserializerArgs;
    /// use massa_models::config::CHAINID;
    /// use massa_models::operation::{compute_operations_hash, OperationIdSerializer};
    /// use massa_signature::KeyPair;
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    ///
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let parents = (0..THREAD_COUNT)
    ///     .map(|i| BlockId::generate_from_hash(Hash::compute_from(&[i])))
    ///     .collect();
    ///
    /// // create block header
    /// let orig_header = BlockHeader::new_verifiable(
    ///     BlockHeader {
    ///         current_version: 0,
    ///         announced_version: None,
    ///         slot: Slot::new(1, 1),
    ///         parents,
    ///         operation_merkle_root: compute_operations_hash(&[], &OperationIdSerializer::new()),
    ///         endorsements: vec![
    ///             Endorsement::new_verifiable(
    ///                 Endorsement {
    ///                     slot: Slot::new(1, 1),
    ///                     index: 1,
    ///                     endorsed_block: BlockId::generate_from_hash(Hash::compute_from(&[1])),
    ///                 },
    ///                 EndorsementSerializer::new(),
    ///                 &keypair,
    ///                 *CHAINID,
    ///                 None
    ///             )
    ///             .unwrap(),
    ///             Endorsement::new_verifiable(
    ///                 Endorsement {
    ///                     slot: Slot::new(1, 1),
    ///                     index: 3,
    ///                     endorsed_block: BlockId::generate_from_hash(Hash::compute_from(&[1])),
    ///                 },
    ///                 EndorsementSerializer::new(),
    ///                 &keypair,
    ///                 *CHAINID,
    ///                 None
    ///             )
    ///             .unwrap(),
    ///         ],
    ///     denunciations: vec![],},
    ///     BlockHeaderSerializer::new(),
    ///     &keypair,
    ///     *CHAINID,
    ///     None
    /// )
    /// .unwrap();
    ///
    /// // create block
    /// let orig_block = Block {
    ///     header: orig_header,
    ///     operations: Vec::new(),
    /// };
    ///
    /// let full_block = Block::new_verifiable(orig_block, BlockSerializer::new(), &keypair, *CHAINID, None).unwrap();
    /// let export_parents = full_block
    ///     .content
    ///     .header
    ///     .content
    ///     .parents
    ///     .iter()
    ///     .enumerate()
    ///     .map(|(n, id)| (*id, n as u64))
    ///     .collect();
    /// let export_active_block = ExportActiveBlock {
    ///    block: full_block.clone(),
    ///    parents: export_parents,
    ///    is_final: false,
    /// };
    ///
    /// let mut serialized = Vec::new();
    /// ExportActiveBlockSerializer::new().serialize(&export_active_block, &mut serialized).unwrap();
    /// let args = BlockDeserializerArgs {
    ///   thread_count: 32, max_operations_per_block: 16, endorsement_count: 1000,max_denunciations_per_block_header: 128,last_start_period: Some(0),chain_id: *CHAINID};
    /// let (rest, export_deserialized) = ExportActiveBlockDeserializer::new(args).deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert_eq!(export_deserialized.block.id, export_active_block.block.id);
    /// assert_eq!(export_deserialized.block.serialized_data, export_active_block.block.serialized_data);
    /// assert_eq!(export_deserialized.parents, export_active_block.parents);
    /// assert_eq!(rest.len(), 0);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], ExportActiveBlock, E> {
        let (rest, (block, parents, is_final)) = context(
            "Failed ExportActiveBlock deserialization",
            tuple((
                // block
                context("Failed block deserialization", |input| {
                    self.sec_share_block_deserializer.deserialize(input)
                }),
                // parents
                context(
                    "Failed parents deserialization",
                    alt((
                        value(Vec::new(), tag(&[0])),
                        preceded(
                            tag(&[1]),
                            count(
                                tuple((
                                    context("Failed block_id deserialization", |input| {
                                        self.block_id_deserializer.deserialize(input)
                                    }),
                                    context("Failed period deserialization", |input| {
                                        self.period_deserializer.deserialize(input)
                                    }),
                                )),
                                self.thread_count as usize,
                            ),
                        ),
                    )),
                ),
                // finality
                context(
                    "Failed is_final deserialization",
                    alt((value(true, tag(&[1])), value(false, tag(&[0])))),
                ),
            )),
        )
        .parse(buffer)?;

        let export = ExportActiveBlock {
            block,
            parents,
            is_final,
        };
        if export
            .check_parents_match_header(self.thread_count)
            .is_err()
        {
            return Err(nom::Err::Failure(ContextError::add_context(
                rest,
                "ExportActiveBlock parents must match signed header parents",
                ParseError::from_error_kind(rest, nom::error::ErrorKind::Fail),
            )));
        }
        Ok((rest, export))
    }
}
