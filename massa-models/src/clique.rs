// Copyright (c) 2022 MASSA LABS <info@massa.net>

use core::usize;

use massa_hash::HashDeserializer;
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U32VarIntDeserializer, U32VarIntSerializer,
    U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::branch::alt;
use nom::bytes::complete::tag;
use nom::combinator::value;
use nom::error::context;
use nom::error::{ContextError, ParseError};
use nom::multi::length_count;
use nom::sequence::tuple;
use nom::{IResult, Parser};
use serde::{Deserialize, Serialize};

use crate::block_id::BlockId;
use crate::prehash::PreHashSet;
use std::ops::Bound::{Excluded, Included};

/// Mutually compatible blocks in the graph
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Clique {
    /// the block ids of the blocks in that clique
    pub block_ids: PreHashSet<BlockId>,
    /// Fitness used to compute finality
    /// Depends on descendants and endorsement count
    pub fitness: u64,
    /// True if it is the clique of higher fitness
    pub is_blockclique: bool,
}

impl Default for Clique {
    fn default() -> Self {
        Clique {
            block_ids: Default::default(),
            fitness: 0,
            is_blockclique: true,
        }
    }
}

/// Basic serializer for `Clique`
#[derive(Default)]
pub(crate) struct CliqueSerializer {
    block_ids_length_serializer: U32VarIntSerializer,
    fitness_serializer: U64VarIntSerializer,
}

impl CliqueSerializer {
    /// Creates a `CliqueSerializer`
    pub(crate) fn new() -> Self {
        Self {
            block_ids_length_serializer: U32VarIntSerializer::new(),
            fitness_serializer: U64VarIntSerializer::new(),
        }
    }
}

impl Serializer<Clique> for CliqueSerializer {
    /// ## Example
    /// ```rust
    /// # use massa_models::clique::{Clique, CliqueSerializer};
    /// # use massa_models::block_id::BlockId;
    /// # use massa_hash::Hash;
    /// # use std::str::FromStr;
    /// # use massa_serialization::Serializer;
    /// # pub fn get_dummy_block_id(s: &str) -> BlockId {
    /// #     BlockId(Hash::compute_from(s.as_bytes()))
    /// # }
    /// let clique = Clique {
    ///         block_ids: vec![get_dummy_block_id("parent1"), get_dummy_block_id("parent2")].into_iter().collect(),
    ///         fitness: 123,
    ///         is_blockclique: true,
    ///     };
    /// let mut buffer = Vec::new();
    /// let mut serializer = CliqueSerializer::new();
    /// serializer.serialize(&clique, &mut buffer).unwrap();
    /// ```
    fn serialize(&self, value: &Clique, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.block_ids_length_serializer
            .serialize(&(value.block_ids.len() as u32), buffer)?;
        for block_id in &value.block_ids {
            buffer.extend(block_id.0.to_bytes())
        }
        self.fitness_serializer.serialize(&value.fitness, buffer)?;
        buffer.push(u8::from(value.is_blockclique));
        Ok(())
    }
}

/// Basic deserializer for `Clique`
pub(crate) struct CliqueDeserializer {
    block_ids_length_deserializer: U32VarIntDeserializer,
    block_id_deserializer: HashDeserializer,
    fitness_deserializer: U64VarIntDeserializer,
}

impl CliqueDeserializer {
    /// Creates a `CliqueDeserializer`
    pub(crate) fn new(max_bootstrap_blocks: u32) -> Self {
        Self {
            block_ids_length_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Excluded(max_bootstrap_blocks),
            ),
            block_id_deserializer: HashDeserializer::new(),
            fitness_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
        }
    }
}

impl Deserializer<Clique> for CliqueDeserializer {
    /// ## Example
    /// ```rust
    /// # use massa_models::clique::{Clique, CliqueDeserializer, CliqueSerializer};
    /// # use massa_models::block_id::BlockId;
    /// # use massa_hash::Hash;
    /// # use std::str::FromStr;
    /// # use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// # pub fn get_dummy_block_id(s: &str) -> BlockId {
    /// #     BlockId(Hash::compute_from(s.as_bytes()))
    /// # }
    /// let clique = Clique {
    ///         block_ids: vec![get_dummy_block_id("parent1"), get_dummy_block_id("parent2")].into_iter().collect(),
    ///         fitness: 123,
    ///         is_blockclique: true,
    ///     };
    /// let mut buffer = Vec::new();
    /// let mut serializer = CliqueSerializer::new();
    /// serializer.serialize(&clique, &mut buffer).unwrap();
    /// let mut deserializer = CliqueDeserializer::new(1000);
    /// let (rest, clique_deserialized) = deserializer.deserialize::<DeserializeError>(&buffer).unwrap();
    /// assert_eq!(clique.block_ids, clique_deserialized.block_ids);
    /// assert_eq!(clique.is_blockclique, clique_deserialized.is_blockclique);
    /// assert_eq!(clique.fitness, clique_deserialized.fitness);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Clique, E> {
        context(
            "Failed Clique deserialization",
            tuple((
                length_count(
                    context("Failed length deserialization", |input| {
                        self.block_ids_length_deserializer.deserialize(input)
                    }),
                    context("Failed block_id deserialization", |input| {
                        self.block_id_deserializer
                            .deserialize(input)
                            .map(|(rest, hash)| (rest, BlockId(hash)))
                    }),
                ),
                context("Failed fitness deserialization", |input| {
                    self.fitness_deserializer.deserialize(input)
                }),
                context(
                    "Failed is_blockclique deserialization",
                    alt((
                        value(true, |input| tag(&[1u8])(input)),
                        value(false, |input| tag(&[0u8])(input)),
                    )),
                ),
            )),
        )
        .map(|(block_ids, fitness, is_blockclique)| Clique {
            block_ids: block_ids.into_iter().collect(),
            fitness,
            is_blockclique,
        })
        .parse(buffer)
    }
}
