use crate::error::ConsensusError;
use massa_hash::HashDeserializer;
use massa_models::{
    active_block::ActiveBlock,
    block::{Block, BlockDeserializer, BlockId, WrappedBlock},
    prehash::PreHashMap,
    wrapped::{WrappedDeserializer, WrappedSerializer},
};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use massa_storage::Storage;
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
    pub block: WrappedBlock,
    /// one `(block id, period)` per thread ( if not genesis )
    pub parents: Vec<(BlockId, u64)>,
    /// for example has its fitness reached the given threshold
    pub is_final: bool,
}

impl ExportActiveBlock {
    /// conversion from active block to export active block
    pub fn from_active_block(a_block: &ActiveBlock, storage: &Storage) -> Self {
        // get block
        let block = storage
            .read_blocks()
            .get(&a_block.block_id)
            .expect("active block missing in storage")
            .clone();

        // TODO: if we decide that endorsements are separate, also gather endorsements here
        ExportActiveBlock {
            parents: a_block.parents.clone(),
            is_final: a_block.is_final,
            block,
        }
    }

    /// consuming conversion from `ExportActiveBlock` to `ActiveBlock`
    pub fn to_active_block(
        self,
        ref_storage: &Storage,
        thread_count: u8,
    ) -> Result<(ActiveBlock, Storage), ConsensusError> {
        // create resulting storage
        let mut storage = ref_storage.clone_without_refs();

        // add endorsements to storage and claim refs
        // TODO change if we decide that endorsements are stored separately
        storage.store_endorsements(self.block.content.header.content.endorsements.clone());

        // Note: the block's parents are not claimed in the block's storage here but on graph inclusion

        // create ActiveBlock
        let active_block = ActiveBlock {
            creator_address: self.block.creator_address,
            block_id: self.block.id,
            parents: self.parents.clone(),
            children: vec![PreHashMap::default(); thread_count as usize], // will be computed once the full graph is available
            descendants: Default::default(), // will be computed once the full graph is available
            is_final: self.is_final,
            slot: self.block.content.header.content.slot,
            fitness: self.block.get_fitness(),
        };

        // add block to storage and claim ref
        storage.store_block(self.block);

        Ok((active_block, storage))
    }
}

/// Basic serializer of `ExportActiveBlock`
#[derive(Default)]
pub struct ExportActiveBlockSerializer {
    wrapped_serializer: WrappedSerializer,
    period_serializer: U64VarIntSerializer,
}

impl ExportActiveBlockSerializer {
    /// Create a new `ExportActiveBlockSerializer`
    pub fn new() -> Self {
        ExportActiveBlockSerializer {
            wrapped_serializer: WrappedSerializer::new(),
            period_serializer: U64VarIntSerializer::new(),
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
        self.wrapped_serializer.serialize(&value.block, buffer)?;

        // parents with periods
        // note: there should be no parents for genesis blocks
        buffer.push(u8::from(!value.parents.is_empty()));
        for (hash, period) in value.parents.iter() {
            buffer.extend(hash.0.to_bytes());
            self.period_serializer.serialize(period, buffer)?;
        }

        // finality
        buffer.push(u8::from(value.is_final));

        Ok(())
    }
}

/// Basic deserializer of `ExportActiveBlock`
pub struct ExportActiveBlockDeserializer {
    wrapped_block_deserializer: WrappedDeserializer<Block, BlockDeserializer>,
    hash_deserializer: HashDeserializer,
    period_deserializer: U64VarIntDeserializer,
    thread_count: u8,
}

impl ExportActiveBlockDeserializer {
    /// Create a new `ExportActiveBlockDeserializer`
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        thread_count: u8,
        endorsement_count: u32,
        max_operations_per_block: u32,
        // IMPORTANT TODO: remove unused args
        _max_datastore_value_length: u64,
        _max_function_name_length: u16,
        _max_parameters_size: u32,
        _max_op_datastore_entry_count: u64,
        _max_op_datastore_key_length: u8,
        _max_op_datastore_value_length: u64,
    ) -> Self {
        ExportActiveBlockDeserializer {
            wrapped_block_deserializer: WrappedDeserializer::new(BlockDeserializer::new(
                thread_count,
                max_operations_per_block,
                endorsement_count,
            )),
            hash_deserializer: HashDeserializer::new(),
            period_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
            thread_count,
        }
    }
}

impl Deserializer<ExportActiveBlock> for ExportActiveBlockDeserializer {
    /// ## Example:
    /// ```rust
    /// use massa_consensus_exports::export_active_block::{ExportActiveBlock, ExportActiveBlockDeserializer, ExportActiveBlockSerializer};
    /// use massa_models::{ledger_models::LedgerChanges, config::THREAD_COUNT, rolls::RollUpdates, block::{BlockId, Block, BlockSerializer, BlockHeader, BlockHeaderSerializer}, prehash::PreHashSet, endorsement::{Endorsement, EndorsementSerializerLW}, slot::Slot, wrapped::WrappedContent};
    /// use massa_hash::Hash;
    /// use std::collections::HashSet;
    /// use massa_signature::KeyPair;
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    ///
    /// let keypair = KeyPair::generate();
    /// let parents = (0..THREAD_COUNT)
    ///     .map(|i| BlockId(Hash::compute_from(&[i])))
    ///     .collect();
    ///
    /// // create block header
    /// let orig_header = BlockHeader::new_wrapped(
    ///     BlockHeader {
    ///         slot: Slot::new(1, 1),
    ///         parents,
    ///         operation_merkle_root: Hash::compute_from("mno".as_bytes()),
    ///         endorsements: vec![
    ///             Endorsement::new_wrapped(
    ///                 Endorsement {
    ///                     slot: Slot::new(1, 1),
    ///                     index: 1,
    ///                     endorsed_block: BlockId(Hash::compute_from("blk1".as_bytes())),
    ///                 },
    ///                 EndorsementSerializerLW::new(),
    ///                 &keypair,
    ///             )
    ///             .unwrap(),
    ///             Endorsement::new_wrapped(
    ///                 Endorsement {
    ///                     slot: Slot::new(4, 0),
    ///                     index: 3,
    ///                     endorsed_block: BlockId(Hash::compute_from("blk2".as_bytes())),
    ///                 },
    ///                 EndorsementSerializerLW::new(),
    ///                 &keypair,
    ///             )
    ///             .unwrap(),
    ///         ],
    ///     },
    ///     BlockHeaderSerializer::new(),
    ///     &keypair,
    /// )
    /// .unwrap();
    ///
    /// // create block
    /// let orig_block = Block {
    ///     header: orig_header,
    ///     operations: Vec::new(),
    /// };
    ///
    /// let full_block = Block::new_wrapped(orig_block, BlockSerializer::new(), &keypair).unwrap();
    /// let export_active_block = ExportActiveBlock {
    ///    block: full_block.clone(),
    ///    parents: vec![],
    ///    is_final: false,
    /// };
    ///
    /// let mut serialized = Vec::new();
    /// ExportActiveBlockSerializer::new().serialize(&export_active_block, &mut serialized).unwrap();
    /// let (rest, export_deserialized) = ExportActiveBlockDeserializer::new(32, 9, 1000, 1000, 1000, 1000, 10, 255, 10_000).deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert_eq!(export_deserialized.block.id, export_active_block.block.id);
    /// assert_eq!(export_deserialized.block.serialized_data, export_active_block.block.serialized_data);
    /// assert_eq!(rest.len(), 0);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], ExportActiveBlock, E> {
        context(
            "Failed ExportActiveBlock deserialization",
            tuple((
                // block
                context("Failed block deserialization", |input| {
                    self.wrapped_block_deserializer.deserialize(input)
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
                                        self.hash_deserializer
                                            .deserialize(input)
                                            .map(|(rest, hash)| (rest, BlockId(hash)))
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
        .map(|(block, parents, is_final)| ExportActiveBlock {
            block,
            parents,
            is_final,
        })
        .parse(buffer)
    }
}
