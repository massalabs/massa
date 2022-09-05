use crate::error::{GraphError, GraphResult as Result};
use massa_hash::HashDeserializer;
use massa_models::{
    active_block::ActiveBlock,
    block::{Block, BlockDeserializer, BlockId, WrappedBlock},
    operation::{Operation, OperationDeserializer, WrappedOperation},
    prehash::{PreHashMap, PreHashSet},
    wrapped::{WrappedDeserializer, WrappedSerializer},
};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U32VarIntDeserializer, U32VarIntSerializer,
    U64VarIntDeserializer, U64VarIntSerializer,
};
use massa_storage::Storage;
use nom::branch::alt;
use nom::{
    bytes::complete::tag,
    combinator::value,
    error::{ContextError, ParseError},
    multi::{count, length_count},
    sequence::{preceded, tuple},
};
use nom::{error::context, IResult, Parser};
use serde::{Deserialize, Serialize};
use tracing::info;
use std::ops::Bound::Included;

/// Exportable version of `ActiveBlock`
/// Fields that can be easily recomputed were left out
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportActiveBlock {
    /// The block.
    pub block: WrappedBlock,
    /// The operations.
    pub operations: Vec<WrappedOperation>,
    /// one `(block id, period)` per thread ( if not genesis )
    pub parents: Vec<(BlockId, u64)>,
    /// for example has its fitness reached the given threshold
    pub is_final: bool,
}

impl ExportActiveBlock {
    /// conversion from active block to export active block
    pub fn from_active_block(a_block: &ActiveBlock, storage: &Storage) -> Self {
        // get block
        println!("AURELIEN: export active block block START");
        let block = storage
            .read_blocks()
            .get(&a_block.block_id)
            .expect("active block missing in storage")
            .clone();
        println!("AURELIEN: export active block block END");
        // get ops
        let operations = {
            println!("AURELIEN: ExportActiveBlock START");
            let read_ops = storage.read_operations();
            block
                .content
                .operations
                .iter()
                .map(|op_id| {
                    read_ops
                        .get(op_id)
                        .expect("active block operation missing in storage")
                        .clone()
                })
                .collect()
        };
        println!("AURELIEN: ExportActiveBlock END");

        // TODO if we deciede that endorsements are separate, also gather endorsements here

        ExportActiveBlock {
            operations,
            parents: a_block.parents.clone(),
            is_final: a_block.is_final,
            block,
        }
    }

    /// consuming conversion from ExportActiveBlock to ActiveBlock
    pub fn to_active_block(
        self,
        ref_storage: &Storage,
        thread_count: u8,
    ) -> Result<(ActiveBlock, Storage), GraphError> {
        // create resulting storage
        let mut storage = ref_storage.clone_without_refs();

        // add operations to storage and claim refs
        storage.store_operations(self.operations);

        // check that the block operations match the stored ones
        if storage.get_op_refs()
            != &self
                .block
                .content
                .operations
                .iter()
                .cloned()
                .collect::<PreHashSet<_>>()
        {
            return Err(GraphError::MissingOperation(
                "operation list mismatch on active block conversion".into(),
            ));
        }

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
    operation_count_serializer: U32VarIntSerializer,
}

impl ExportActiveBlockSerializer {
    /// Create a new `ExportActiveBlockSerializer`
    pub fn new() -> Self {
        ExportActiveBlockSerializer {
            wrapped_serializer: WrappedSerializer::new(),
            period_serializer: U64VarIntSerializer::new(),
            operation_count_serializer: U32VarIntSerializer::new(),
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

        // operations
        self.operation_count_serializer.serialize(
            &value
                .operations
                .len()
                .try_into()
                .map_err(|_| SerializeError::NumberTooBig("Too many operations".to_string()))?,
            buffer,
        )?;
        for op in &value.operations {
            self.wrapped_serializer.serialize(op, buffer)?;
        }

        // parents with periods
        // note: there should be no parents for genesis blocks
        buffer.push(if value.parents.is_empty() { 0u8 } else { 1u8 });
        for (hash, period) in value.parents.iter() {
            buffer.extend(hash.0.to_bytes());
            self.period_serializer.serialize(period, buffer)?;
        }

        // finality
        buffer.push(if value.is_final { 1u8 } else { 0u8 });

        Ok(())
    }
}

/// Basic deserializer of `ExportActiveBlock`
pub struct ExportActiveBlockDeserializer {
    wrapped_block_deserializer: WrappedDeserializer<Block, BlockDeserializer>,
    wrapped_operation_deserializer: WrappedDeserializer<Operation, OperationDeserializer>,
    hash_deserializer: HashDeserializer,
    period_deserializer: U64VarIntDeserializer,
    operation_count_serializer: U32VarIntDeserializer,
    thread_count: u8,
}

impl ExportActiveBlockDeserializer {
    /// Create a new `ExportActiveBlockDeserializer`
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        thread_count: u8,
        endorsement_count: u32,
        max_operations_per_block: u32,
        max_datastore_value_length: u64,
        max_function_name_length: u16,
        max_parameters_size: u32,
    ) -> Self {
        ExportActiveBlockDeserializer {
            wrapped_block_deserializer: WrappedDeserializer::new(BlockDeserializer::new(
                thread_count,
                max_operations_per_block,
                endorsement_count,
            )),
            wrapped_operation_deserializer: WrappedDeserializer::new(OperationDeserializer::new(
                max_datastore_value_length,
                max_function_name_length,
                max_parameters_size,
            )),
            operation_count_serializer: U32VarIntDeserializer::new(
                Included(0),
                Included(max_operations_per_block),
            ),
            hash_deserializer: HashDeserializer::new(),
            period_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
            thread_count,
        }
    }
}

impl Deserializer<ExportActiveBlock> for ExportActiveBlockDeserializer {
    /// ## Example:
    /// ```rust
    /// use massa_graph::export_active_block::{ExportActiveBlock, ExportActiveBlockDeserializer, ExportActiveBlockSerializer};
    /// use massa_models::{ledger_models::LedgerChanges, config::THREAD_COUNT, rolls::RollUpdates, block::{BlockId, Block, BlockSerializer, BlockHeader, BlockHeaderSerializer}, prehash::PreHashSet, endorsement::{Endorsement, EndorsementSerializer}, slot::Slot, wrapped::WrappedContent};
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
    ///                 EndorsementSerializer::new(),
    ///                 &keypair,
    ///             )
    ///             .unwrap(),
    ///             Endorsement::new_wrapped(
    ///                 Endorsement {
    ///                     slot: Slot::new(4, 0),
    ///                     index: 3,
    ///                     endorsed_block: BlockId(Hash::compute_from("blk2".as_bytes())),
    ///                 },
    ///                 EndorsementSerializer::new(),
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
    ///    operations: vec![],
    ///    is_final: false,
    /// };
    ///
    /// let mut serialized = Vec::new();
    /// ExportActiveBlockSerializer::new().serialize(&export_active_block, &mut serialized).unwrap();
    /// let (rest, export_deserialized) = ExportActiveBlockDeserializer::new(32, 9, 1000, 1000, 1000, 1000, ).deserialize::<DeserializeError>(&serialized).unwrap();
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
                // operations
                context(
                    "Failed operations deserialization",
                    length_count(
                        context("Failed operation count deserialization", |input| {
                            self.operation_count_serializer.deserialize(input)
                        }),
                        context("Failed operation deserialization", |input| {
                            self.wrapped_operation_deserializer.deserialize(input)
                        }),
                    ),
                ),
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
        .map(|(block, operations, parents, is_final)| ExportActiveBlock {
            block,
            operations,
            parents,
            is_final,
        })
        .parse(buffer)
    }
}
