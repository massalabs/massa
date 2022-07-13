use crate::error::{GraphError, GraphResult as Result};
use massa_hash::HashDeserializer;
use massa_models::{
    active_block::ActiveBlock,
    constants::{default::MAX_BOOTSTRAP_CHILDREN, *},
    ledger_models::{LedgerChangeDeserializer, LedgerChangeSerializer, LedgerChanges},
    prehash::{Map, Set},
    rolls::{RollUpdateDeserializer, RollUpdateSerializer, RollUpdates},
    wrapped::{WrappedDeserializer, WrappedSerializer},
    *,
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
use std::ops::Bound::Included;

/// Exportable version of `ActiveBlock`
/// Fields that can be easily recomputed were left out
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportActiveBlock {
    /// The block.
    pub block: WrappedBlock,
    /// The Id of the block.
    pub block_id: BlockId,
    /// one `(block id, period)` per thread ( if not genesis )
    pub parents: Vec<(BlockId, u64)>,
    /// one `HashMap<Block id, period>` per thread (blocks that need to be kept)
    /// Children reference that block as a parent
    pub children: Vec<Map<BlockId, u64>>,
    /// dependencies required for validity check
    pub dependencies: Set<BlockId>,
    /// for example has its fitness reached the given threshold
    pub is_final: bool,
    /// Changes caused by this block
    pub block_ledger_changes: LedgerChanges,
    /// `Address -> RollUpdate`
    pub roll_updates: RollUpdates,
    /// list of `(period, address, did_create)` for all block/endorsement creation events
    pub production_events: Vec<(u64, Address, bool)>,
}

impl TryFrom<ExportActiveBlock> for ActiveBlock {
    fn try_from(a_block: ExportActiveBlock) -> Result<ActiveBlock> {
        let operation_set = a_block
            .block
            .content
            .operations
            .iter()
            .enumerate()
            .map(|(idx, op)| (op.id, (idx, op.content.expire_period)))
            .collect();

        let endorsement_ids = a_block
            .block
            .content
            .header
            .content
            .endorsements
            .iter()
            .map(|endo| (endo.id, endo.content.index))
            .collect();

        let addresses_to_operations = a_block.block.involved_addresses(&operation_set)?;
        let addresses_to_endorsements = a_block.block.addresses_to_endorsements()?;
        Ok(ActiveBlock {
            creator_address: a_block.block.creator_address,
            block_id: a_block.block_id,
            parents: a_block.parents.clone(),
            children: a_block.children.clone(),
            dependencies: a_block.dependencies.clone(),
            descendants: Default::default(), // will be computed once the full graph is available
            is_final: a_block.is_final,
            block_ledger_changes: a_block.block_ledger_changes.clone(),
            operation_set,
            endorsement_ids,
            addresses_to_operations,
            roll_updates: a_block.roll_updates.clone(),
            production_events: a_block.production_events.clone(),
            addresses_to_endorsements,
            slot: a_block.block.content.header.content.slot,
        })
    }
    type Error = GraphError;
}

impl ExportActiveBlock {
    /// try conversion from active block to export active block
    pub fn try_from_active_block(a_block: &ActiveBlock, storage: Storage) -> Result<Self> {
        let block = storage.retrieve_block(&a_block.block_id).ok_or_else(|| {
            GraphError::MissingBlock(format!(
                "missing block ExportActiveBlock::try_from_active_block: {}",
                a_block.block_id
            ))
        })?;
        let stored_block = block.read();
        Ok(ExportActiveBlock {
            block: stored_block.clone(),
            block_id: a_block.block_id,
            parents: a_block.parents.clone(),
            children: a_block.children.clone(),
            dependencies: a_block.dependencies.clone(),
            is_final: a_block.is_final,
            block_ledger_changes: a_block.block_ledger_changes.clone(),
            roll_updates: a_block.roll_updates.clone(),
            production_events: a_block.production_events.clone(),
        })
    }
}

/// Basic serializer of `ExportActiveBlock`
#[derive(Default)]
pub struct ExportActiveBlockSerializer {
    wrapped_serializer: WrappedSerializer,
    period_serializer: U64VarIntSerializer,
    length_serializer: U32VarIntSerializer,
    ledger_change_serializer: LedgerChangeSerializer,
    roll_update_serializer: RollUpdateSerializer,
}

impl ExportActiveBlockSerializer {
    /// Create a new `ExportActiveBlockSerializer`
    pub fn new() -> Self {
        ExportActiveBlockSerializer {
            wrapped_serializer: WrappedSerializer::new(),
            period_serializer: U64VarIntSerializer::new(),
            length_serializer: U32VarIntSerializer::new(),
            ledger_change_serializer: LedgerChangeSerializer::new(),
            roll_update_serializer: RollUpdateSerializer::new(),
        }
    }
}

impl Serializer<ExportActiveBlock> for ExportActiveBlockSerializer {
    fn serialize(
        &self,
        value: &ExportActiveBlock,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        buffer.push(if value.is_final { 1 } else { 0 });
        self.wrapped_serializer.serialize(&value.block, buffer)?;
        // note: there should be none if slot period=0
        buffer.push(if value.parents.is_empty() { 0 } else { 1 });
        for (hash, period) in value.parents.iter() {
            buffer.extend(hash.0.to_bytes());
            self.period_serializer.serialize(period, buffer)?;
        }
        self.length_serializer.serialize(
            &value
                .children
                .len()
                .try_into()
                .map_err(|_| SerializeError::NumberTooBig("Too many children".to_string()))?,
            buffer,
        )?;
        for map in value.children.iter() {
            self.length_serializer.serialize(
                &map.len().try_into().map_err(|_| {
                    SerializeError::NumberTooBig("Too block in children map".to_string())
                })?,
                buffer,
            )?;
            for (hash, period) in map.iter() {
                buffer.extend(hash.0.to_bytes());
                self.period_serializer.serialize(period, buffer)?;
            }
        }
        self.length_serializer.serialize(
            &value
                .dependencies
                .len()
                .try_into()
                .map_err(|_| SerializeError::NumberTooBig("Too many dependencies".to_string()))?,
            buffer,
        )?;
        for dep in value.dependencies.iter() {
            buffer.extend(dep.0.to_bytes());
        }
        self.length_serializer.serialize(
            &value.block_ledger_changes.0.len().try_into().map_err(|_| {
                SerializeError::NumberTooBig("Too many block_ledger_change".to_string())
            })?,
            buffer,
        )?;
        for (addr, change) in value.block_ledger_changes.0.iter() {
            buffer.extend(addr.to_bytes());
            self.ledger_change_serializer.serialize(change, buffer)?;
        }
        self.length_serializer.serialize(
            &value
                .roll_updates
                .0
                .len()
                .try_into()
                .map_err(|_| SerializeError::NumberTooBig("Too many roll_updates".to_string()))?,
            buffer,
        )?;
        for (addr, roll_update) in value.roll_updates.0.iter() {
            buffer.extend(addr.to_bytes());
            self.roll_update_serializer.serialize(roll_update, buffer)?;
        }
        self.length_serializer.serialize(
            &value.production_events.len().try_into().map_err(|_| {
                SerializeError::NumberTooBig("Too many production_events".to_string())
            })?,
            buffer,
        )?;
        for (period, addr, has_created) in value.production_events.iter() {
            self.period_serializer.serialize(period, buffer)?;
            buffer.extend(addr.to_bytes());
            buffer.push(if *has_created { 1u8 } else { 0u8 });
        }
        Ok(())
    }
}

/// Basic deserializer of `ExportActiveBlock`
pub struct ExportActiveBlockDeserializer {
    wrapped_block_deserializer: WrappedDeserializer<Block, BlockDeserializer>,
    hash_deserializer: HashDeserializer,
    period_deserializer: U64VarIntDeserializer,
    children_length_deserializer: U32VarIntDeserializer,
    map_length_deserializer: U32VarIntDeserializer,
    dependencies_length_deserializer: U32VarIntDeserializer,
    block_ledger_changes_length_deserializer: U32VarIntDeserializer,
    ledger_change_deserializer: LedgerChangeDeserializer,
    address_deserializer: AddressDeserializer,
    roll_updates_length_deserializer: U32VarIntDeserializer,
    roll_update_deserializer: RollUpdateDeserializer,
    production_events_deserializer: U32VarIntDeserializer,
}

impl ExportActiveBlockDeserializer {
    /// Create a new `ExportActiveBlockDeserializer`
    pub fn new() -> Self {
        #[cfg(not(feature = "sandbox"))]
        let thread_count = THREAD_COUNT;
        #[cfg(feature = "sandbox")]
        let thread_count = *THREAD_COUNT;
        ExportActiveBlockDeserializer {
            wrapped_block_deserializer: WrappedDeserializer::new(BlockDeserializer::new()),
            hash_deserializer: HashDeserializer::new(),
            period_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
            children_length_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(thread_count as u32),
            ),
            map_length_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(MAX_BOOTSTRAP_CHILDREN),
            ),
            dependencies_length_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(MAX_BOOTSTRAP_DEPS),
            ),
            block_ledger_changes_length_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(u32::MAX),
            ),
            ledger_change_deserializer: LedgerChangeDeserializer::new(),
            address_deserializer: AddressDeserializer::new(),
            roll_updates_length_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(MAX_BOOTSTRAP_POS_ENTRIES),
            ),
            roll_update_deserializer: RollUpdateDeserializer::new(),
            production_events_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(u32::MAX),
            ),
        }
    }
}

impl Default for ExportActiveBlockDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<ExportActiveBlock> for ExportActiveBlockDeserializer {
    /// ## Example:
    /// ```rust
    /// use massa_graph::{ExportActiveBlock, ExportActiveBlockDeserializer, ExportActiveBlockSerializer};
    ///
    /// let export_active_block = ExportActiveBlock {
    ///    block: Block {
    ///       hash: Hash::compute_from("23tuSEWed8WoEasjboGxKi4qRtM7qFJnnp4QrsuASmNnk81GnH"),
    ///       period: 1,
    /// }
    ///       parents: vec![],
    ///       children: vec![(Hash::compute_from("23tuSEWed8WoEasjboGxKi4qRtM7qFJnnp4QrsuASmNnk81GnH"), 1)],
    ///       dependencies: vec![Hash::compute_from("23tuSEWed8WoEasjboGxKi4qRtM7qFJnnp4QrsuASmNnk81GnH")],
    ///       is_final: false,
    ///       block_ledger_changes: BlockLedgerChanges {
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], ExportActiveBlock, E> {
        #[cfg(not(feature = "sandbox"))]
        let thread_count = THREAD_COUNT;
        #[cfg(feature = "sandbox")]
        let thread_count = *THREAD_COUNT;
        context(
            "Failed ExportActiveBlock deserialization",
            tuple((
                context(
                    "Failed is_final deserialization",
                    alt((value(true, tag(&[1])), value(false, tag(&[0])))),
                ),
                context("Failed block deserialization", |input| {
                    self.wrapped_block_deserializer.deserialize(input)
                }),
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
                                thread_count as usize,
                            ),
                        ),
                    )),
                ),
                context(
                    "Failed children deserialization",
                    length_count(
                        context("Failed length deserialization", |input| {
                            self.children_length_deserializer.deserialize(input)
                        }),
                        length_count(
                            context("Failed length deserialization", |input| {
                                self.map_length_deserializer.deserialize(input)
                            }),
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
                        ),
                    ),
                ),
                context(
                    "Failed dependencies deserialization",
                    length_count(
                        context("Failed length deserialization", |input| {
                            self.dependencies_length_deserializer.deserialize(input)
                        }),
                        context("Failed block_id deserialization", |input| {
                            self.hash_deserializer
                                .deserialize(input)
                                .map(|(rest, hash)| (rest, BlockId(hash)))
                        }),
                    ),
                ),
                context(
                    "Failed block_ledger_changes deserialization",
                    length_count(
                        context("Failed length deserialization", |input| {
                            self.block_ledger_changes_length_deserializer
                                .deserialize(input)
                        }),
                        tuple((
                            context("Failed address deserialization", |input| {
                                self.address_deserializer.deserialize(input)
                            }),
                            context("Failed ledger_change deserialization", |input| {
                                self.ledger_change_deserializer.deserialize(input)
                            }),
                        )),
                    ),
                ),
                context(
                    "Failed roll_updates deserialization",
                    length_count(
                        context("Failed length deserialization", |input| {
                            self.roll_updates_length_deserializer.deserialize(input)
                        }),
                        tuple((
                            context("Failed address deserialization", |input| {
                                self.address_deserializer.deserialize(input)
                            }),
                            context("Failed roll_update deserialization", |input| {
                                self.roll_update_deserializer.deserialize(input)
                            }),
                        )),
                    ),
                ),
                context(
                    "Failed production_events deserialization",
                    length_count(
                        context("Failed length deserialization", |input| {
                            self.production_events_deserializer.deserialize(input)
                        }),
                        tuple((
                            context("Failed period deserialization", |input| {
                                self.period_deserializer.deserialize(input)
                            }),
                            context("Failed address deserialization", |input| {
                                self.address_deserializer.deserialize(input)
                            }),
                            context(
                                "Failed did_create deserialization",
                                alt((value(true, tag(&[1])), value(false, tag(&[0])))),
                            ),
                        )),
                    ),
                ),
            )),
        )
        .map(
            |(
                is_final,
                block,
                parents,
                children,
                dependencies,
                block_ledger_changes,
                roll_updates,
                production_events,
            )| ExportActiveBlock {
                is_final,
                block_id: block.id,
                block,
                parents,
                children: children
                    .into_iter()
                    .map(|map| map.into_iter().collect())
                    .collect(),
                dependencies: dependencies.into_iter().collect(),
                block_ledger_changes: LedgerChanges(block_ledger_changes.into_iter().collect()),
                roll_updates: RollUpdates(roll_updates.into_iter().collect()),
                production_events,
            },
        )
        .parse(buffer)
    }
}
