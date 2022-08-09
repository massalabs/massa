use massa_hash::HashDeserializer;
use massa_models::{
    clique::{Clique, CliqueDeserializer, CliqueSerializer},
    prehash::{Map, Set},
    BlockId,
};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U32VarIntDeserializer, U32VarIntSerializer,
    U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{error::context, multi::length_count, sequence::tuple, IResult, Parser};
use nom::{
    error::{ContextError, ParseError},
    multi::count,
};
use serde::{Deserialize, Serialize};

use crate::{
    export_active_block::{
        ExportActiveBlock, ExportActiveBlockDeserializer, ExportActiveBlockSerializer,
    },
    ledger::{
        ConsensusLedgerSubset, ConsensusLedgerSubsetDeserializer, ConsensusLedgerSubsetSerializer,
    },
};
use std::ops::Bound::Included;

/// Bootstrap graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapableGraph {
    /// Map of active blocks, were blocks are in their exported version.
    pub active_blocks: Map<BlockId, ExportActiveBlock>,
    /// Best parents hashes in each thread.
    pub best_parents: Vec<(BlockId, u64)>,
    /// Latest final period and block hash in each thread.
    pub latest_final_blocks_periods: Vec<(BlockId, u64)>,
    /// Head of the incompatibility graph.
    pub gi_head: Map<BlockId, Set<BlockId>>,
    /// List of maximal cliques of compatible blocks.
    pub max_cliques: Vec<Clique>,
    /// Ledger at last final blocks
    pub ledger: ConsensusLedgerSubset,
}

/// Basic serializer for `BootstrapableGraph`
#[derive(Default)]
pub struct BootstrapableGraphSerializer {
    length_serializer: U32VarIntSerializer,
    export_active_block_serializer: ExportActiveBlockSerializer,
    period_serializer: U64VarIntSerializer,
    clique_serializer: CliqueSerializer,
    consensus_ledger_subset_serializer: ConsensusLedgerSubsetSerializer,
}

impl BootstrapableGraphSerializer {
    /// Creates a `BootstrapableGraphSerializer`
    pub fn new() -> Self {
        Self {
            length_serializer: U32VarIntSerializer::new(),
            export_active_block_serializer: ExportActiveBlockSerializer::new(),
            period_serializer: U64VarIntSerializer::new(),
            clique_serializer: CliqueSerializer::new(),
            consensus_ledger_subset_serializer: ConsensusLedgerSubsetSerializer::new(),
        }
    }
}

impl Serializer<BootstrapableGraph> for BootstrapableGraphSerializer {
    /// ## Example
    /// ```rust
    /// use massa_graph::{BootstrapableGraph, BootstrapableGraphSerializer, ledger::ConsensusLedgerSubset};
    /// use massa_serialization::Serializer;
    /// use massa_hash::Hash;
    /// use massa_models::{prehash::Map, BlockId, constants::THREAD_COUNT};
    /// let mut bootstrapable_graph = BootstrapableGraph {
    ///   active_blocks: Map::default(),
    ///   best_parents: Vec::new(),
    ///   latest_final_blocks_periods: Vec::new(),
    ///   gi_head: Map::default(),
    ///   max_cliques: Vec::new(),
    ///   ledger: ConsensusLedgerSubset::default(),
    /// };
    /// for i in 0..THREAD_COUNT {
    ///   bootstrapable_graph.best_parents.push((BlockId(Hash::compute_from("nvqFJ38Gixhv5Pf3QaJofXa2kW37bHtnk2QKaA6ZRycJLv8FH".as_bytes())), 7));
    ///   bootstrapable_graph.latest_final_blocks_periods.push((BlockId(Hash::compute_from("nvqFJ38Gixhv5Pf3QaJofXa2kW37bHtnk2QKaA6ZRycJLv8FH".as_bytes())), 10));
    /// }
    /// let mut buffer = Vec::new();
    /// BootstrapableGraphSerializer::new().serialize(&bootstrapable_graph, &mut buffer).unwrap();
    /// ```
    fn serialize(
        &self,
        value: &BootstrapableGraph,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        self.length_serializer.serialize(
            &value
                .active_blocks
                .len()
                .try_into()
                .map_err(|_| SerializeError::NumberTooBig("Too much active_block".to_string()))?,
            buffer,
        )?;
        for (block_id, export_active_block) in value.active_blocks.iter() {
            buffer.extend(block_id.0.to_bytes());
            self.export_active_block_serializer
                .serialize(export_active_block, buffer)?;
        }
        for (parent_h, parent_period) in value.best_parents.iter() {
            buffer.extend(parent_h.0.to_bytes());
            self.period_serializer.serialize(parent_period, buffer)?;
        }
        for (hash, period) in value.latest_final_blocks_periods.iter() {
            buffer.extend(hash.0.to_bytes());
            self.period_serializer.serialize(period, buffer)?;
        }
        self.length_serializer.serialize(
            &value
                .gi_head
                .len()
                .try_into()
                .map_err(|_| SerializeError::NumberTooBig("Too much gi_head".to_string()))?,
            buffer,
        )?;
        for (block_id, gi_head) in &value.gi_head {
            buffer.extend(block_id.0.to_bytes());
            self.length_serializer.serialize(
                &gi_head.len().try_into().map_err(|_| {
                    SerializeError::NumberTooBig("Too much entry in gi_head".to_string())
                })?,
                buffer,
            )?;
            for gi_head_block_id in gi_head.iter() {
                buffer.extend(gi_head_block_id.0.to_bytes());
            }
        }
        self.length_serializer.serialize(
            &value
                .max_cliques
                .len()
                .try_into()
                .map_err(|_| SerializeError::NumberTooBig("Too much max_cliques".to_string()))?,
            buffer,
        )?;
        for clique in &value.max_cliques {
            self.clique_serializer.serialize(clique, buffer)?;
        }
        self.consensus_ledger_subset_serializer
            .serialize(&value.ledger, buffer)?;
        Ok(())
    }
}

/// Basic deserializer for `BootstrapableGraph`
pub struct BootstrapableGraphDeserializer {
    blocks_length_deserializer: U32VarIntDeserializer,
    export_active_block_deserializer: ExportActiveBlockDeserializer,
    period_deserializer: U64VarIntDeserializer,
    set_length_deserializer: U32VarIntDeserializer,
    clique_length_deserializer: U32VarIntDeserializer,
    clique_deserializer: CliqueDeserializer,
    consensus_ledger_data_deserializer: ConsensusLedgerSubsetDeserializer,
    hash_deserializer: HashDeserializer,
    thread_count: u8,
}

impl BootstrapableGraphDeserializer {
    /// Creates a `BootstrapableGraphDeserializer`
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        thread_count: u8,
        endorsement_count: u32,
        max_bootstrap_blocks: u32,
        max_bootstrap_cliques: u32,
        max_bootstrap_children: u32,
        max_bootstrap_deps: u32,
        max_bootstrap_pos_entries: u32,
        max_operations_per_block: u32,
        max_ledger_changes_per_slot: u32,
        max_production_events_per_block: u32,
    ) -> Self {
        Self {
            blocks_length_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(max_bootstrap_blocks),
            ),
            export_active_block_deserializer: ExportActiveBlockDeserializer::new(
                thread_count,
                endorsement_count,
                max_bootstrap_children,
                max_bootstrap_deps,
                max_bootstrap_pos_entries,
                max_operations_per_block,
                max_ledger_changes_per_slot,
                max_production_events_per_block,
            ),
            period_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
            set_length_deserializer: U32VarIntDeserializer::new(Included(0), Included(u32::MAX)),
            clique_length_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(max_bootstrap_cliques),
            ),
            clique_deserializer: CliqueDeserializer::new(max_bootstrap_blocks),
            consensus_ledger_data_deserializer: ConsensusLedgerSubsetDeserializer::new(),
            hash_deserializer: HashDeserializer::new(),
            thread_count,
        }
    }
}

impl Deserializer<BootstrapableGraph> for BootstrapableGraphDeserializer {
    /// ## Example
    /// ```rust
    /// use massa_graph::{BootstrapableGraph, BootstrapableGraphDeserializer, BootstrapableGraphSerializer, ledger::ConsensusLedgerSubset};
    /// use massa_serialization::{Deserializer, Serializer, DeserializeError};
    /// use massa_hash::Hash;
    /// use massa_models::{prehash::Map, BlockId, constants::THREAD_COUNT};
    /// let mut bootstrapable_graph = BootstrapableGraph {
    ///   active_blocks: Map::default(),
    ///   best_parents: Vec::new(),
    ///   latest_final_blocks_periods: Vec::new(),
    ///   gi_head: Map::default(),
    ///   max_cliques: Vec::new(),
    ///   ledger: ConsensusLedgerSubset::default(),
    /// };
    /// for i in 0..32 {
    ///   bootstrapable_graph.best_parents.push((BlockId(Hash::compute_from("nvqFJ38Gixhv5Pf3QaJofXa2kW37bHtnk2QKaA6ZRycJLv8FH".as_bytes())), 7));
    ///   bootstrapable_graph.latest_final_blocks_periods.push((BlockId(Hash::compute_from("nvqFJ38Gixhv5Pf3QaJofXa2kW37bHtnk2QKaA6ZRycJLv8FH".as_bytes())), 10));
    /// }
    /// let mut buffer = Vec::new();
    /// BootstrapableGraphSerializer::new().serialize(&bootstrapable_graph, &mut buffer).unwrap();
    /// let (rest, bootstrapable_graph_deserialized) = BootstrapableGraphDeserializer::new(32, 9, 10, 10, 100, 1000, 1000, 1000, 10000, 10000).deserialize::<DeserializeError>(&buffer).unwrap();
    /// let mut buffer2 = Vec::new();
    /// BootstrapableGraphSerializer::new().serialize(&bootstrapable_graph_deserialized, &mut buffer2).unwrap();
    /// assert_eq!(buffer, buffer2);
    /// assert_eq!(rest.len(), 0);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BootstrapableGraph, E> {
        context(
            "Failed BootstrapableGraph deserialization",
            tuple((
                context(
                    "Failed active_blocks deserialization",
                    length_count(
                        context("Failed length deserialization", |input| {
                            self.blocks_length_deserializer.deserialize(input)
                        }),
                        context(
                            "Failed export_active_block deserialization",
                            tuple((
                                |input| self.hash_deserializer.deserialize(input),
                                |input| self.export_active_block_deserializer.deserialize(input),
                            )),
                        ),
                    ),
                ),
                context(
                    "Failed best_parents deserialization",
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
                context(
                    "Failed latest_final_blocks_periods deserialization",
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
                context(
                    "Failed gi_head deserialization",
                    length_count(
                        context("Failed key length deserialization", |input| {
                            self.blocks_length_deserializer.deserialize(input)
                        }),
                        tuple((
                            context("Failed key block_id deserialization", |input| {
                                self.hash_deserializer
                                    .deserialize(input)
                                    .map(|(rest, hash)| (rest, BlockId(hash)))
                            }),
                            length_count(
                                context("Failed value length deserialization", |input| {
                                    self.set_length_deserializer.deserialize(input)
                                }),
                                context("Failed value block_id deserialization", |input| {
                                    self.hash_deserializer
                                        .deserialize(input)
                                        .map(|(rest, hash)| (rest, BlockId(hash)))
                                }),
                            ),
                        )),
                    ),
                ),
                context(
                    "Failed max_cliques deserialization",
                    length_count(
                        context("Failed length deserialization", |input| {
                            self.clique_length_deserializer.deserialize(input)
                        }),
                        context("Failed clique deserialization", |input| {
                            self.clique_deserializer.deserialize(input)
                        }),
                    ),
                ),
                context("Failed ledger deserialization", |input| {
                    self.consensus_ledger_data_deserializer.deserialize(input)
                }),
            )),
        )
        .map(
            |(
                active_blocks,
                best_parents,
                latest_final_blocks_periods,
                gi_head,
                max_cliques,
                ledger,
            )| {
                BootstrapableGraph {
                    active_blocks: active_blocks
                        .into_iter()
                        .map(|(hash, value)| (BlockId(hash), value))
                        .collect(),
                    best_parents,
                    latest_final_blocks_periods,
                    gi_head: gi_head
                        .into_iter()
                        .map(|(id, set)| (id, set.into_iter().collect()))
                        .collect(),
                    max_cliques,
                    ledger,
                }
            },
        )
        .parse(buffer)
    }
}
