use massa_hash::HashDeserializer;
use massa_models::{
    clique::{Clique, CliqueDeserializer, CliqueSerializer},
    constants::{MAX_BOOTSTRAP_BLOCKS, MAX_BOOTSTRAP_CLIQUES, THREAD_COUNT},
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
        for (block_id, export_active_block) in &value.active_blocks {
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
}

impl BootstrapableGraphDeserializer {
    /// Creates a `BootstrapableGraphDeserializer`
    pub fn new() -> Self {
        Self {
            blocks_length_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(MAX_BOOTSTRAP_BLOCKS),
            ),
            export_active_block_deserializer: ExportActiveBlockDeserializer::new(),
            period_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
            set_length_deserializer: U32VarIntDeserializer::new(Included(0), Included(u32::MAX)),
            clique_length_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(MAX_BOOTSTRAP_CLIQUES),
            ),
            clique_deserializer: CliqueDeserializer::new(),
            consensus_ledger_data_deserializer: ConsensusLedgerSubsetDeserializer::new(),
            hash_deserializer: HashDeserializer::new(),
        }
    }
}

impl Default for BootstrapableGraphDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<BootstrapableGraph> for BootstrapableGraphDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BootstrapableGraph, E> {
        #[cfg(feature = "sandbox")]
        let thread_count = *THREAD_COUNT;
        #[cfg(not(feature = "sandbox"))]
        let thread_count = THREAD_COUNT;
        context(
            "Failed BootstrapableGraph deserialization",
            tuple((
                length_count(
                    |input| self.blocks_length_deserializer.deserialize(input),
                    |input| self.export_active_block_deserializer.deserialize(input),
                ),
                count(
                    tuple((
                        |input| {
                            self.hash_deserializer
                                .deserialize(input)
                                .map(|(rest, hash)| (rest, BlockId(hash)))
                        },
                        |input| self.period_deserializer.deserialize(input),
                    )),
                    thread_count as usize,
                ),
                count(
                    tuple((
                        |input| {
                            self.hash_deserializer
                                .deserialize(input)
                                .map(|(rest, hash)| (rest, BlockId(hash)))
                        },
                        |input| self.period_deserializer.deserialize(input),
                    )),
                    thread_count as usize,
                ),
                length_count(
                    |input| self.blocks_length_deserializer.deserialize(input),
                    tuple((
                        |input| {
                            self.hash_deserializer
                                .deserialize(input)
                                .map(|(rest, hash)| (rest, BlockId(hash)))
                        },
                        length_count(
                            |input| self.set_length_deserializer.deserialize(input),
                            |input| {
                                self.hash_deserializer
                                    .deserialize(input)
                                    .map(|(rest, hash)| (rest, BlockId(hash)))
                            },
                        ),
                    )),
                ),
                length_count(
                    |input| self.clique_length_deserializer.deserialize(input),
                    |input| self.clique_deserializer.deserialize(input),
                ),
                |input| self.consensus_ledger_data_deserializer.deserialize(input),
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
                        .map(|export_active_block| {
                            (export_active_block.block_id, export_active_block)
                        })
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
