// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::settings::{BootstrapServerMessageDeserializerArgs, BootstrapClientConfig};
use massa_consensus_exports::bootstrapable_graph::{
    BootstrapableGraph, BootstrapableGraphDeserializer, BootstrapableGraphSerializer,
};
use massa_consensus_exports::export_active_block::ExportActiveBlock;
use massa_db_exports::StreamBatch;
use massa_hash::Hash;
use massa_models::block::{Block, BlockSerializer};
use massa_models::block_header::{BlockHeader, BlockHeaderSerializer};
use massa_models::block_id::{BlockId, BlockIdDeserializer, BlockIdSerializer, BlockIdV0};
use massa_models::config::{ENDORSEMENT_COUNT, MAX_OPERATIONS_PER_BLOCK, MAX_DENUNCIATIONS_PER_BLOCK_HEADER};
use massa_models::endorsement::{self, Endorsement, EndorsementSerializer};
use massa_models::prehash::{PreHashSet, PreHashed};
use massa_models::secure_share::{SecureShare, SecureShareContent};
use massa_models::serialization::{
    PreHashSetDeserializer, PreHashSetSerializer, VecU8Deserializer, VecU8Serializer,
};
use massa_models::slot::{Slot, SlotDeserializer, SlotSerializer};
use massa_models::streaming_step::{
    StreamingStep, StreamingStepDeserializer, StreamingStepSerializer,
};
use massa_models::version::{Version, VersionDeserializer, VersionSerializer};
use massa_protocol_exports::{
    BootstrapPeers, BootstrapPeersDeserializer, BootstrapPeersSerializer, PeerId,
};
use massa_serialization::{
    BoolDeserializer, BoolSerializer, Deserializer, OptionDeserializer, OptionSerializer,
    SerializeError, Serializer, U32VarIntDeserializer, U32VarIntSerializer, U64VarIntDeserializer,
    U64VarIntSerializer,
};
use massa_signature::KeyPair;
use massa_time::{MassaTime, MassaTimeDeserializer, MassaTimeSerializer};
use nom::error::context;
use nom::multi::{length_count, length_data};
use nom::sequence::tuple;
use nom::Parser;
use nom::{
    error::{ContextError, ParseError},
    IResult,
};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use rand::prelude::Distribution;
use rand::{SeedableRng, Rng};
use rand::rngs::SmallRng;
use std::collections::{HashMap, BTreeMap};
use std::convert::TryInto;
use std::ops::Bound::{Excluded, Included};
use std::str::FromStr;
use std::time::Duration;
use std::unreachable;

/// Messages used during bootstrap by server
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum BootstrapServerMessage {
    /// Sync clocks
    BootstrapTime {
        /// The current time on the bootstrap server.
        server_time: MassaTime,
        /// The version of the bootstrap server.
        version: Version,
    },
    /// Bootstrap peers
    BootstrapPeers {
        /// Server peers
        peers: BootstrapPeers,
    },
    /// Part of final state and consensus
    BootstrapPart {
        /// Slot the state changes are attached to
        slot: Slot,
        /// Part of the state in a serialized way
        state_part: StreamBatch<Slot>,
        /// Part of the state (specific to versioning) in a serialized way
        versioning_part: StreamBatch<Slot>,
        /// Part of the consensus graph
        consensus_part: BootstrapableGraph,
        /// Outdated block ids in the current consensus graph bootstrap
        consensus_outdated_ids: PreHashSet<BlockId>,
        /// Last Start Period for network restart management
        last_start_period: Option<u64>,
        /// Last Slot before downtime for network restart management
        last_slot_before_downtime: Option<Option<Slot>>,
    },
    /// Message sent when the final state and consensus bootstrap are finished
    BootstrapFinished,
    /// Slot sent to get state changes is too old
    SlotTooOld,
    /// Bootstrap error
    BootstrapError {
        /// Error message
        error: String,
    },
}

impl ToString for BootstrapServerMessage {
    fn to_string(&self) -> String {
        match self {
            BootstrapServerMessage::BootstrapTime { .. } => "BootstrapTime".to_string(),
            BootstrapServerMessage::BootstrapPeers { .. } => "BootstrapPeers".to_string(),
            BootstrapServerMessage::BootstrapPart { .. } => "BootstrapPart".to_string(),
            BootstrapServerMessage::BootstrapFinished => "BootstrapFinished".to_string(),
            BootstrapServerMessage::SlotTooOld => "SlotTooOld".to_string(),
            BootstrapServerMessage::BootstrapError { error } => {
                format!("BootstrapError {{ error: {} }}", error)
            }
        }
    }
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u32)]
enum MessageServerTypeId {
    BootstrapTime = 0u32,
    Peers = 1u32,
    FinalStatePart = 2u32,
    FinalStateFinished = 3u32,
    SlotTooOld = 4u32,
    BootstrapError = 5u32,
}

/// Serializer for `BootstrapServerMessage`
pub struct BootstrapServerMessageSerializer {
    u32_serializer: U32VarIntSerializer,
    u64_serializer: U64VarIntSerializer,
    time_serializer: MassaTimeSerializer,
    version_serializer: VersionSerializer,
    peers_serializer: BootstrapPeersSerializer,
    bootstrapable_graph_serializer: BootstrapableGraphSerializer,
    block_id_set_serializer: PreHashSetSerializer<BlockId, BlockIdSerializer>,
    vec_u8_serializer: VecU8Serializer,
    opt_vec_u8_serializer: OptionSerializer<Vec<u8>, VecU8Serializer>,
    slot_serializer: SlotSerializer,
    opt_last_start_period_serializer: OptionSerializer<u64, U64VarIntSerializer>,
    opt_last_slot_before_downtime_serializer:
        OptionSerializer<Option<Slot>, OptionSerializer<Slot, SlotSerializer>>,
}

impl Default for BootstrapServerMessageSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl BootstrapServerMessageSerializer {
    /// Creates a new `BootstrapServerMessageSerializer`
    pub fn new() -> Self {
        Self {
            u32_serializer: U32VarIntSerializer::new(),
            u64_serializer: U64VarIntSerializer::new(),
            time_serializer: MassaTimeSerializer::new(),
            version_serializer: VersionSerializer::new(),
            peers_serializer: BootstrapPeersSerializer::new(),
            bootstrapable_graph_serializer: BootstrapableGraphSerializer::new(),
            block_id_set_serializer: PreHashSetSerializer::new(BlockIdSerializer::new()),
            vec_u8_serializer: VecU8Serializer::new(),
            opt_vec_u8_serializer: OptionSerializer::new(VecU8Serializer::new()),
            slot_serializer: SlotSerializer::new(),
            opt_last_start_period_serializer: OptionSerializer::new(U64VarIntSerializer::new()),
            opt_last_slot_before_downtime_serializer: OptionSerializer::new(OptionSerializer::new(
                SlotSerializer::new(),
            )),
        }
    }
}

impl Serializer<BootstrapServerMessage> for BootstrapServerMessageSerializer {
    /// ## Example
    /// ```rust
    /// use massa_bootstrap::{BootstrapServerMessage, BootstrapServerMessageSerializer};
    /// use massa_serialization::Serializer;
    /// use massa_time::MassaTime;
    /// use massa_models::version::Version;
    /// use std::str::FromStr;
    ///
    /// let message_serializer = BootstrapServerMessageSerializer::new();
    /// let bootstrap_server_message = BootstrapServerMessage::BootstrapTime {
    ///    server_time: MassaTime::from_millis(0),
    ///    version: Version::from_str("TEST.1.10").unwrap(),
    /// };
    /// let mut message_serialized = Vec::new();
    /// message_serializer.serialize(&bootstrap_server_message, &mut message_serialized).unwrap();
    /// ```
    fn serialize(
        &self,
        value: &BootstrapServerMessage,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        match value {
            BootstrapServerMessage::BootstrapTime {
                server_time,
                version,
            } => {
                self.u32_serializer
                    .serialize(&u32::from(MessageServerTypeId::BootstrapTime), buffer)?;
                self.time_serializer.serialize(server_time, buffer)?;
                self.version_serializer.serialize(version, buffer)?;
            }
            BootstrapServerMessage::BootstrapPeers { peers } => {
                self.u32_serializer
                    .serialize(&u32::from(MessageServerTypeId::Peers), buffer)?;
                self.peers_serializer.serialize(peers, buffer)?;
            }
            BootstrapServerMessage::BootstrapPart {
                slot,
                state_part,
                versioning_part,
                consensus_part,
                consensus_outdated_ids,
                last_start_period,
                last_slot_before_downtime,
            } => {
                // message type
                self.u32_serializer
                    .serialize(&u32::from(MessageServerTypeId::FinalStatePart), buffer)?;
                // slot
                self.slot_serializer.serialize(slot, buffer)?;
                // state
                self.u64_serializer
                    .serialize(&(state_part.new_elements.len() as u64), buffer)?;
                for (key, value) in state_part.new_elements.iter() {
                    self.vec_u8_serializer.serialize(key, buffer)?;
                    self.vec_u8_serializer.serialize(value, buffer)?;
                }
                self.u64_serializer.serialize(
                    &(state_part.updates_on_previous_elements.len() as u64),
                    buffer,
                )?;
                for (key, value) in state_part.updates_on_previous_elements.iter() {
                    self.vec_u8_serializer.serialize(key, buffer)?;
                    self.opt_vec_u8_serializer.serialize(value, buffer)?;
                }
                self.slot_serializer
                    .serialize(&state_part.change_id, buffer)?;
                // versioning
                self.u64_serializer
                    .serialize(&(versioning_part.new_elements.len() as u64), buffer)?;
                for (key, value) in versioning_part.new_elements.iter() {
                    self.vec_u8_serializer.serialize(key, buffer)?;
                    self.vec_u8_serializer.serialize(value, buffer)?;
                }
                self.u64_serializer.serialize(
                    &(versioning_part.updates_on_previous_elements.len() as u64),
                    buffer,
                )?;
                for (key, value) in versioning_part.updates_on_previous_elements.iter() {
                    self.vec_u8_serializer.serialize(key, buffer)?;
                    self.opt_vec_u8_serializer.serialize(value, buffer)?;
                }
                self.slot_serializer
                    .serialize(&versioning_part.change_id, buffer)?;
                // consensus graph
                self.bootstrapable_graph_serializer
                    .serialize(consensus_part, buffer)?;
                // consensus outdated ids
                self.block_id_set_serializer
                    .serialize(consensus_outdated_ids, buffer)?;
                // initial state
                self.opt_last_start_period_serializer
                    .serialize(last_start_period, buffer)?;
                // initial state
                self.opt_last_slot_before_downtime_serializer
                    .serialize(last_slot_before_downtime, buffer)?;
            }
            BootstrapServerMessage::BootstrapFinished => {
                self.u32_serializer
                    .serialize(&u32::from(MessageServerTypeId::FinalStateFinished), buffer)?;
            }
            BootstrapServerMessage::SlotTooOld => {
                self.u32_serializer
                    .serialize(&u32::from(MessageServerTypeId::SlotTooOld), buffer)?;
            }
            BootstrapServerMessage::BootstrapError { error } => {
                self.u32_serializer
                    .serialize(&u32::from(MessageServerTypeId::BootstrapError), buffer)?;
                self.u32_serializer.serialize(
                    &error.len().try_into().map_err(|_| {
                        SerializeError::GeneralError("Fail to convert usize to u32".to_string())
                    })?,
                    buffer,
                )?;
                buffer.extend(error.as_bytes())
            }
        }
        Ok(())
    }
}

/// Deserializer for `BootstrapServerMessage`
pub struct BootstrapServerMessageDeserializer {
    message_id_deserializer: U32VarIntDeserializer,
    time_deserializer: MassaTimeDeserializer,
    version_deserializer: VersionDeserializer,
    peers_deserializer: BootstrapPeersDeserializer,
    state_new_elements_length_deserializer: U64VarIntDeserializer,
    state_updates_length_deserializer: U64VarIntDeserializer,
    vec_u8_deserializer: VecU8Deserializer,
    opt_vec_u8_deserializer: OptionDeserializer<Vec<u8>, VecU8Deserializer>,
    bootstrapable_graph_deserializer: BootstrapableGraphDeserializer,
    block_id_set_deserializer: PreHashSetDeserializer<BlockId, BlockIdDeserializer>,
    length_bootstrap_error: U64VarIntDeserializer,
    slot_deserializer: SlotDeserializer,
    opt_last_start_period_deserializer: OptionDeserializer<u64, U64VarIntDeserializer>,
    opt_last_slot_before_downtime_deserializer:
        OptionDeserializer<Option<Slot>, OptionDeserializer<Slot, SlotDeserializer>>,
}

impl BootstrapServerMessageDeserializer {
    /// Creates a new `BootstrapServerMessageDeserializer`
    #[allow(clippy::too_many_arguments)]
    pub fn new(args: BootstrapServerMessageDeserializerArgs) -> Self {
        Self {
            message_id_deserializer: U32VarIntDeserializer::new(Included(0), Included(u32::MAX)),
            time_deserializer: MassaTimeDeserializer::new((
                Included(MassaTime::from_millis(0)),
                Included(MassaTime::from_millis(u64::MAX)),
            )),
            version_deserializer: VersionDeserializer::new(),
            peers_deserializer: BootstrapPeersDeserializer::new(
                args.max_advertise_length,
                args.max_listeners_per_peer,
            ),
            vec_u8_deserializer: VecU8Deserializer::new(
                Included(0),
                Included(args.max_datastore_value_length),
            ),
            opt_vec_u8_deserializer: OptionDeserializer::new(VecU8Deserializer::new(
                Included(0),
                Included(args.max_datastore_value_length),
            )),
            bootstrapable_graph_deserializer: BootstrapableGraphDeserializer::new(
                (&args).into(),
                args.max_bootstrap_blocks_length,
            ),
            block_id_set_deserializer: PreHashSetDeserializer::new(
                BlockIdDeserializer::new(),
                Included(0),
                Included(args.max_bootstrap_blocks_length as u64),
            ),
            length_bootstrap_error: U64VarIntDeserializer::new(
                Included(0),
                Included(args.max_bootstrap_error_length),
            ),
            state_new_elements_length_deserializer: U64VarIntDeserializer::new(
                Included(0),
                Included(args.max_new_elements_size)
            ),
            state_updates_length_deserializer: U64VarIntDeserializer::new(
                Included(0),
                Included(u64::MAX),
            ),
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(args.thread_count)),
            ),
            opt_last_start_period_deserializer: OptionDeserializer::new(
                U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
            ),
            opt_last_slot_before_downtime_deserializer: OptionDeserializer::new(
                OptionDeserializer::new(SlotDeserializer::new(
                    (Included(0), Included(u64::MAX)),
                    (Included(0), Excluded(args.thread_count)),
                )),
            ),
        }
    }
}

impl Deserializer<BootstrapServerMessage> for BootstrapServerMessageDeserializer {
    /// ## Example
    /// ```rust
    /// use massa_bootstrap::{BootstrapServerMessage, BootstrapServerMessageSerializer, BootstrapServerMessageDeserializer};
    /// use massa_bootstrap::BootstrapServerMessageDeserializerArgs;
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use massa_time::MassaTime;
    /// use massa_models::version::Version;
    /// use std::str::FromStr;
    ///
    /// let message_serializer = BootstrapServerMessageSerializer::new();
    /// let args = BootstrapServerMessageDeserializerArgs {
    ///     thread_count: 32, endorsement_count: 16,
    ///     max_listeners_per_peer: 1000,
    ///     max_advertise_length: 1000, max_bootstrap_blocks_length: 1000,
    ///     max_operations_per_block: 1000, max_new_elements: 1000,
    ///     max_async_pool_changes: 1000, max_async_pool_length: 1000, max_async_message_data: 1000,
    ///     max_ledger_changes_count: 1000, max_datastore_key_length: 255,
    ///     max_datastore_value_length: 1000,
    ///     max_datastore_entry_count: 1000, max_bootstrap_error_length: 1000, max_changes_slot_count: 1000,
    ///     max_rolls_length: 1000, max_production_stats_length: 1000, max_credits_length: 1000,
    ///     max_executed_ops_length: 1000, max_ops_changes_length: 1000,
    ///     mip_store_stats_block_considered: 100,
    ///     max_denunciations_per_block_header: 128, max_denunciation_changes_length: 1000,};
    /// let message_deserializer = BootstrapServerMessageDeserializer::new(args);
    /// let bootstrap_server_message = BootstrapServerMessage::BootstrapTime {
    ///    server_time: MassaTime::from_millis(0),
    ///    version: Version::from_str("TEST.1.10").unwrap(),
    /// };
    /// let mut message_serialized = Vec::new();
    /// message_serializer.serialize(&bootstrap_server_message, &mut message_serialized).unwrap();
    /// let (rest, message_deserialized) = message_deserializer.deserialize::<DeserializeError>(&message_serialized).unwrap();
    /// match message_deserialized {
    ///     BootstrapServerMessage::BootstrapTime {
    ///        server_time,
    ///        version,
    ///    } => {
    ///     assert_eq!(server_time, MassaTime::from_millis(0));
    ///     assert_eq!(version, Version::from_str("TEST.1.10").unwrap());
    ///   }
    ///   _ => panic!("Unexpected message"),
    /// }
    /// assert_eq!(rest.len(), 0);
    /// ```
    fn deserialize<'a, E: nom::error::ParseError<&'a [u8]> + nom::error::ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BootstrapServerMessage, E> {
        context("Failed BootstrapServerMessage deserialization", |buffer| {
            let (input, id) = context("Failed id deserialization", |input| {
                self.message_id_deserializer.deserialize(input)
            })
            .map(|id| {
                MessageServerTypeId::try_from(id).map_err(|_| {
                    nom::Err::Error(ParseError::from_error_kind(
                        buffer,
                        nom::error::ErrorKind::Eof,
                    ))
                })
            })
            .parse(buffer)?;
            match id? {
                MessageServerTypeId::BootstrapTime => tuple((
                    context("Failed server_time deserialization", |input| {
                        self.time_deserializer.deserialize(input)
                    }),
                    context("Failed version deserialization", |input| {
                        self.version_deserializer.deserialize(input)
                    }),
                ))
                .map(
                    |(server_time, version)| BootstrapServerMessage::BootstrapTime {
                        server_time,
                        version,
                    },
                )
                .parse(input),
                MessageServerTypeId::Peers => context("Failed peers deserialization", |input| {
                    self.peers_deserializer.deserialize(input)
                })
                .map(|peers| BootstrapServerMessage::BootstrapPeers { peers })
                .parse(input),
                MessageServerTypeId::FinalStatePart => tuple((
                    context("Failed slot deserialization", |input| {
                        self.slot_deserializer.deserialize(input)
                    }),
                    context(
                        "Failed state_part deserialization",
                        tuple((
                            context(
                                "Failed new_elements deserialization",
                                length_count(
                                    context("Failed length deserialization", |input| {
                                        self.state_new_elements_length_deserializer
                                            .deserialize(input)
                                    }),
                                    tuple((
                                        |input| self.vec_u8_deserializer.deserialize(input),
                                        |input| self.vec_u8_deserializer.deserialize(input),
                                    )),
                                ),
                            ),
                            context(
                                "Failed updates deserialization",
                                length_count(
                                    context("Failed length deserialization", |input| {
                                        self.state_updates_length_deserializer.deserialize(input)
                                    }),
                                    tuple((
                                        |input| self.vec_u8_deserializer.deserialize(input),
                                        |input| self.opt_vec_u8_deserializer.deserialize(input),
                                    )),
                                ),
                            ),
                            context("Failed slot deserialization", |input| {
                                self.slot_deserializer.deserialize(input)
                            }),
                        )),
                    ),
                    context(
                        "Failed versioning_part deserialization",
                        tuple((
                            context(
                                "Failed new_elements deserialization",
                                length_count(
                                    context("Failed length deserialization", |input| {
                                        self.state_new_elements_length_deserializer
                                            .deserialize(input)
                                    }),
                                    tuple((
                                        |input| self.vec_u8_deserializer.deserialize(input),
                                        |input| self.vec_u8_deserializer.deserialize(input),
                                    )),
                                ),
                            ),
                            context(
                                "Failed updates deserialization",
                                length_count(
                                    context("Failed length deserialization", |input| {
                                        self.state_updates_length_deserializer.deserialize(input)
                                    }),
                                    tuple((
                                        |input| self.vec_u8_deserializer.deserialize(input),
                                        |input| self.opt_vec_u8_deserializer.deserialize(input),
                                    )),
                                ),
                            ),
                            context("Failed slot deserialization", |input| {
                                self.slot_deserializer.deserialize(input)
                            }),
                        )),
                    ),
                    context("Failed consensus_part deserialization", |input| {
                        self.bootstrapable_graph_deserializer.deserialize(input)
                    }),
                    context("Failed consensus_outdated_ids deserialization", |input| {
                        self.block_id_set_deserializer.deserialize(input)
                    }),
                    context("Failed last_start_period deserialization", |input| {
                        self.opt_last_start_period_deserializer.deserialize(input)
                    }),
                    context(
                        "Failed last_slot_before_downtime deserialization",
                        |input| {
                            self.opt_last_slot_before_downtime_deserializer
                                .deserialize(input)
                        },
                    ),
                ))
                .map(
                    |(
                        slot,
                        (state_part_new_elems, state_part_updates, state_part_change_id),
                        (
                            versioning_part_new_elems,
                            versioning_part_updates,
                            versioning_part_change_id,
                        ),
                        consensus_part,
                        consensus_outdated_ids,
                        last_start_period,
                        last_slot_before_downtime,
                    )| {
                        let state_part = StreamBatch::<Slot> {
                            new_elements: state_part_new_elems.into_iter().collect(),
                            updates_on_previous_elements: state_part_updates.into_iter().collect(),
                            change_id: state_part_change_id,
                        };
                        let versioning_part = StreamBatch::<Slot> {
                            new_elements: versioning_part_new_elems.into_iter().collect(),
                            updates_on_previous_elements: versioning_part_updates
                                .into_iter()
                                .collect(),
                            change_id: versioning_part_change_id,
                        };

                        BootstrapServerMessage::BootstrapPart {
                            slot,
                            state_part,
                            versioning_part,
                            consensus_part,
                            consensus_outdated_ids,
                            last_start_period,
                            last_slot_before_downtime,
                        }
                    },
                )
                .parse(input),
                MessageServerTypeId::FinalStateFinished => {
                    Ok((input, BootstrapServerMessage::BootstrapFinished))
                }
                MessageServerTypeId::SlotTooOld => Ok((input, BootstrapServerMessage::SlotTooOld)),
                MessageServerTypeId::BootstrapError => context(
                    "Failed BootstrapError deserialization",
                    length_data(context("Failed length deserialization", |input| {
                        self.length_bootstrap_error.deserialize(input)
                    })),
                )
                .map(|error| BootstrapServerMessage::BootstrapError {
                    error: String::from_utf8_lossy(error).into_owned(),
                })
                .parse(input),
            }
        })
        .parse(buffer)
    }
}

/// Messages used during bootstrap by client
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum BootstrapClientMessage {
    /// Ask for bootstrap peers
    AskBootstrapPeers,
    /// Ask for a final state and consensus part
    AskBootstrapPart {
        /// Slot we are attached to for changes
        last_slot: Option<Slot>,
        /// Last received state key
        last_state_step: StreamingStep<Vec<u8>>,
        /// Last received versioning key
        last_versioning_step: StreamingStep<Vec<u8>>,
        /// Last received consensus block slot
        last_consensus_step: StreamingStep<PreHashSet<BlockId>>,
        /// Should be true only for the first part, false later
        send_last_start_period: bool,
    },
    /// Bootstrap error
    BootstrapError {
        /// Error message
        error: String,
    },
    /// Bootstrap succeed
    BootstrapSuccess,
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u32)]
enum MessageClientTypeId {
    AskBootstrapPeers = 0u32,
    AskFinalStatePart = 1u32,
    BootstrapError = 2u32,
    BootstrapSuccess = 3u32,
}

/// Serializer for `BootstrapClientMessage`
pub struct BootstrapClientMessageSerializer {
    u32_serializer: U32VarIntSerializer,
    slot_serializer: SlotSerializer,
    state_step_serializer: StreamingStepSerializer<Vec<u8>, VecU8Serializer>,
    block_ids_step_serializer: StreamingStepSerializer<
        PreHashSet<BlockId>,
        PreHashSetSerializer<BlockId, BlockIdSerializer>,
    >,
    bool_serializer: BoolSerializer,
}

impl BootstrapClientMessageSerializer {
    /// Creates a new `BootstrapClientMessageSerializer`
    pub fn new() -> Self {
        Self {
            u32_serializer: U32VarIntSerializer::new(),
            slot_serializer: SlotSerializer::new(),
            state_step_serializer: StreamingStepSerializer::new(VecU8Serializer::new()),
            block_ids_step_serializer: StreamingStepSerializer::new(PreHashSetSerializer::new(
                BlockIdSerializer::new(),
            )),
            bool_serializer: BoolSerializer::new(),
        }
    }
}

impl Default for BootstrapClientMessageSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<BootstrapClientMessage> for BootstrapClientMessageSerializer {
    /// ## Example
    /// ```rust
    /// use massa_bootstrap::{BootstrapClientMessage, BootstrapClientMessageSerializer};
    /// use massa_serialization::Serializer;
    /// use massa_time::MassaTime;
    /// use massa_models::version::Version;
    /// use std::str::FromStr;
    ///
    /// let message_serializer = BootstrapClientMessageSerializer::new();
    /// let bootstrap_server_message = BootstrapClientMessage::AskBootstrapPeers;
    /// let mut message_serialized = Vec::new();
    /// message_serializer.serialize(&bootstrap_server_message, &mut message_serialized).unwrap();
    /// ```
    fn serialize(
        &self,
        value: &BootstrapClientMessage,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        match value {
            BootstrapClientMessage::AskBootstrapPeers => {
                self.u32_serializer
                    .serialize(&u32::from(MessageClientTypeId::AskBootstrapPeers), buffer)?;
            }
            BootstrapClientMessage::AskBootstrapPart {
                last_slot,
                last_state_step,
                last_versioning_step,
                last_consensus_step,
                send_last_start_period,
            } => {
                self.u32_serializer
                    .serialize(&u32::from(MessageClientTypeId::AskFinalStatePart), buffer)?;
                if let Some(slot) = last_slot {
                    self.slot_serializer.serialize(slot, buffer)?;
                    self.state_step_serializer
                        .serialize(last_state_step, buffer)?;
                    self.state_step_serializer
                        .serialize(last_versioning_step, buffer)?;
                    self.block_ids_step_serializer
                        .serialize(last_consensus_step, buffer)?;
                    self.bool_serializer
                        .serialize(send_last_start_period, buffer)?;
                }
            }
            BootstrapClientMessage::BootstrapError { error } => {
                self.u32_serializer
                    .serialize(&u32::from(MessageClientTypeId::BootstrapError), buffer)?;
                self.u32_serializer.serialize(
                    &error.len().try_into().map_err(|_| {
                        SerializeError::GeneralError("Fail to convert usize to u32".to_string())
                    })?,
                    buffer,
                )?;
                buffer.extend(error.as_bytes())
            }
            BootstrapClientMessage::BootstrapSuccess => {
                self.u32_serializer
                    .serialize(&u32::from(MessageClientTypeId::BootstrapSuccess), buffer)?;
            }
        }
        Ok(())
    }
}

/// Deserializer for `BootstrapClientMessage`
pub struct BootstrapClientMessageDeserializer {
    id_deserializer: U32VarIntDeserializer,
    length_error_deserializer: U32VarIntDeserializer,
    slot_deserializer: SlotDeserializer,
    state_step_deserializer: StreamingStepDeserializer<Vec<u8>, VecU8Deserializer>,
    block_ids_step_deserializer: StreamingStepDeserializer<
        PreHashSet<BlockId>,
        PreHashSetDeserializer<BlockId, BlockIdDeserializer>,
    >,
    bool_deserializer: BoolDeserializer,
}

impl BootstrapClientMessageDeserializer {
    /// Creates a new `BootstrapClientMessageDeserializer`
    pub fn new(
        thread_count: u8,
        max_datastore_value_length: u8,
        max_consensus_block_ids: u64,
    ) -> Self {
        Self {
            id_deserializer: U32VarIntDeserializer::new(Included(0), Included(u32::MAX)),
            length_error_deserializer: U32VarIntDeserializer::new(Included(0), Included(100000)),
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            state_step_deserializer: StreamingStepDeserializer::new(VecU8Deserializer::new(
                Included(0),
                Included(max_datastore_value_length as u64),
            )),
            block_ids_step_deserializer: StreamingStepDeserializer::new(
                PreHashSetDeserializer::new(
                    BlockIdDeserializer::new(),
                    Included(0),
                    Included(max_consensus_block_ids),
                ),
            ),
            bool_deserializer: BoolDeserializer::new(),
        }
    }
}

impl Deserializer<BootstrapClientMessage> for BootstrapClientMessageDeserializer {
    /// ## Example
    /// ```rust
    /// use massa_bootstrap::{BootstrapClientMessage, BootstrapClientMessageSerializer, BootstrapClientMessageDeserializer};
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use massa_time::MassaTime;
    /// use massa_models::version::Version;
    /// use std::str::FromStr;
    ///
    /// let message_serializer = BootstrapClientMessageSerializer::new();
    /// let message_deserializer = BootstrapClientMessageDeserializer::new(32, 255, 50);
    /// let bootstrap_server_message = BootstrapClientMessage::AskBootstrapPeers;
    /// let mut message_serialized = Vec::new();
    /// message_serializer.serialize(&bootstrap_server_message, &mut message_serialized).unwrap();
    /// let (rest, message_deserialized) = message_deserializer.deserialize::<DeserializeError>(&message_serialized).unwrap();
    /// match message_deserialized {
    ///     BootstrapClientMessage::AskBootstrapPeers => (),
    ///   _ => panic!("Unexpected message"),
    /// };
    /// assert_eq!(rest.len(), 0);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BootstrapClientMessage, E> {
        context("Failed BootstrapClientMessage deserialization", |buffer| {
            let (input, id) = context("Failed id deserialization", |input| {
                self.id_deserializer.deserialize(input)
            })
            .map(|id| {
                MessageClientTypeId::try_from(id).map_err(|_| {
                    nom::Err::Error(ParseError::from_error_kind(
                        buffer,
                        nom::error::ErrorKind::Eof,
                    ))
                })
            })
            .parse(buffer)?;
            match id? {
                MessageClientTypeId::AskBootstrapPeers => {
                    Ok((input, BootstrapClientMessage::AskBootstrapPeers))
                }
                MessageClientTypeId::AskFinalStatePart => {
                    if input.is_empty() {
                        Ok((
                            input,
                            BootstrapClientMessage::AskBootstrapPart {
                                last_slot: None,
                                last_state_step: StreamingStep::Started,
                                last_versioning_step: StreamingStep::Started,
                                last_consensus_step: StreamingStep::Started,
                                send_last_start_period: true,
                            },
                        ))
                    } else {
                        tuple((
                            context("Failed last_slot deserialization", |input| {
                                self.slot_deserializer.deserialize(input)
                            }),
                            context("Faild last_state_step deserialization", |input| {
                                self.state_step_deserializer.deserialize(input)
                            }),
                            context("Faild last_versioning_step deserialization", |input| {
                                self.state_step_deserializer.deserialize(input)
                            }),
                            context("Failed last_consensus_step deserialization", |input| {
                                self.block_ids_step_deserializer.deserialize(input)
                            }),
                            context("Failed send_last_start_period deserialization", |input| {
                                self.bool_deserializer.deserialize(input)
                            }),
                        ))
                        .map(
                            |(
                                last_slot,
                                last_state_step,
                                last_versioning_step,
                                last_consensus_step,
                                send_last_start_period,
                            )| {
                                BootstrapClientMessage::AskBootstrapPart {
                                    last_slot: Some(last_slot),
                                    last_state_step,
                                    last_versioning_step,
                                    last_consensus_step,
                                    send_last_start_period,
                                }
                            },
                        )
                        .parse(input)
                    }
                }
                MessageClientTypeId::BootstrapError => context(
                    "Failed BootstrapError deserialization",
                    length_data(context("Failed length deserialization", |input| {
                        self.length_error_deserializer.deserialize(input)
                    })),
                )
                .map(|error| BootstrapClientMessage::BootstrapError {
                    error: String::from_utf8_lossy(error).into_owned(),
                })
                .parse(input),
                MessageClientTypeId::BootstrapSuccess => {
                    Ok((input, BootstrapClientMessage::BootstrapSuccess))
                }
            }
        })
        .parse(buffer)
    }
}

impl BootstrapServerMessage {
    pub fn generate<R: Rng>(rng: &mut R) -> Self {
        let variant = rng.gen_range(0..6);
        match variant {
            0 => {
                let t: u64 = rng.gen();
                // Taken from INSTANCE_LEN in version.rs
                let vi: String = (0..4).map(|_| char::from_u32(rng.gen_range(65..91)).unwrap()).collect();
                let major: u32 = rng.gen();
                let minor: u32 = rng.gen();
                let version = Version::from_str(format!("{}.{}.{}", vi, major, minor).as_str()).unwrap();
                let server_time = MassaTime::from_millis(t);
                BootstrapServerMessage::BootstrapTime { server_time, version, }
            },
            1 => {
                let peer_nb = rng.gen_range(0..100);
                let mut peers_list = vec![];
                for _ in 0..peer_nb {
                    peers_list.push(
                        (PeerId::from_public_key(KeyPair::generate(0).unwrap().get_public_key()),
                         HashMap::new()));
                }
                let peers = BootstrapPeers(peers_list);
                BootstrapServerMessage::BootstrapPeers { peers, }
            },
            2 => {
                let state_part = gen_new_stream_batch(rng);
                let versioning_part = gen_new_stream_batch(rng);
                let mut final_blocks = vec![];
                let block_nb = rng.gen_range(0..1000);
                for _ in 0..block_nb {
                    final_blocks.push(gen_export_active_blocks(rng));
                }
                let nb = rng.gen_range(0..1000);
                let mut consensus_outdated_ids = PreHashSet::default();
                for _ in 0..nb {
                    consensus_outdated_ids.insert(gen_random_block_id(rng));
                }
                let consensus_part = BootstrapableGraph { final_blocks, };
                let last_start_period = rng.gen();
                let last_slot_before_downtime = if rng.gen_bool(0.5) {
                    Some(Some(Slot { period: rng.gen(), thread: rng.gen() }))
                } else {
                    None
                };
                let slot = gen_random_slot(rng);
                BootstrapServerMessage::BootstrapPart { slot, state_part, versioning_part, consensus_part, consensus_outdated_ids, last_start_period, last_slot_before_downtime }
            },
            3 => BootstrapServerMessage::BootstrapFinished,
            4 => BootstrapServerMessage::SlotTooOld,
            5 => {
                let mut c = vec![];
                let n = rng.gen_range(0..250);
                for _ in 0..n {
                    c.push(rng.gen::<char>());
                }
                BootstrapServerMessage::BootstrapError { error: c.into_iter().collect() }
            }
            _ => unreachable!(),
        }
    }

    pub fn equals(&self, other: &BootstrapServerMessage) -> bool {
        match (self, other) {
            (BootstrapServerMessage::BootstrapTime { server_time: t1, version: v1 }, BootstrapServerMessage::BootstrapTime { server_time: t2, version: v2 }) =>
                (t1 == t2) && (v1 == v2),
            (BootstrapServerMessage::BootstrapPeers { peers: p1 }, BootstrapServerMessage::BootstrapPeers { peers: p2 }) => p1 == p2,
            (BootstrapServerMessage::BootstrapPart { slot: s1, state_part: state1, versioning_part: v1, consensus_part: c1, consensus_outdated_ids: co1, last_start_period: lp1, last_slot_before_downtime: ls1 }, BootstrapServerMessage::BootstrapPart { slot: s2, state_part: state2, versioning_part: v2, consensus_part: c2, consensus_outdated_ids: co2, last_start_period: lp2, last_slot_before_downtime: ls2 }) => {
                let state_equal = stream_batch_equal(state1, state2);
                let versionning_equal = stream_batch_equal(v1, v2);
                let mut consensus_equal = true;
                for active_block1 in c1.final_blocks.iter() {
                    for active_block2 in c2.final_blocks.iter() {
                        consensus_equal &= active_block1.parents == active_block2.parents;
                        consensus_equal &= active_block1.is_final == active_block2.is_final;
                        consensus_equal &= active_block1.block.serialized_data == active_block2.block.serialized_data;
                    }
                }
                (s1 == s2) && state_equal && versionning_equal && consensus_equal && (co1 == co2) && (lp1 == lp2) && (ls1 == ls2)
            },
            (BootstrapServerMessage::BootstrapFinished, BootstrapServerMessage::BootstrapFinished) => true,
            (BootstrapServerMessage::SlotTooOld, BootstrapServerMessage::SlotTooOld) => true,
            (BootstrapServerMessage::BootstrapError { error: e1 }, BootstrapServerMessage::BootstrapError { error: e2 }) => e1 == e2,
            _ => false
        }
    }
}

fn gen_random_slot<R: Rng>(rng: &mut R) -> Slot {
    Slot { period: rng.gen(), thread: rng.gen_range(0..32) }
}

fn gen_random_block_id<R: Rng>(rng: &mut R) -> BlockId {
    let data = gen_random_vector(256, rng);
    BlockId::BlockIdV0(BlockIdV0(Hash::compute_from(&data)))
}

fn gen_random_block<R: Rng>(rng: &mut R) -> Block {
    let keypair = KeyPair::generate(0).unwrap();
    let slot = gen_random_slot(rng);
    let data = gen_random_vector(256, rng);
    let block_id = gen_random_block_id(rng);
    let mut endorsements = vec![];
    for index in 0..rng.gen_range(1..ENDORSEMENT_COUNT) {
        let end_slot = gen_random_slot(rng);
        let endorsement = Endorsement {
            index,
            slot: end_slot,
            endorsed_block: block_id,
        };
        endorsements.push(endorsement.new_verifiable(EndorsementSerializer::new(), &keypair).unwrap());
    }

    let mut denunciations = vec![];
    for _ in 0..rng.gen_range(0..MAX_DENUNCIATIONS_PER_BLOCK_HEADER) {
        // TODO    Denunciations generation
    }

    let header = BlockHeader {
        current_version: rng.gen(),
        announced_version: rng.gen(),
        slot,
        parents: (0..32).map(|_| gen_random_block_id(rng)).collect(),
        operation_merkle_root: Hash::compute_from(&data),
        endorsements,
        denunciations,
    }.new_verifiable(BlockHeaderSerializer::new(), &keypair).unwrap();
    let mut operations = vec![];
    for _ in 0..rng.gen_range(0..MAX_OPERATIONS_PER_BLOCK) {
        // TODO    Generate operations
    }
    Block {
        header,
        operations,
    }
}

fn gen_export_active_blocks<R: Rng>(rng: &mut R) -> ExportActiveBlock {
    let keypair = KeyPair::generate(0).unwrap();
    let block = gen_random_block(rng).new_verifiable(BlockSerializer::new(), &keypair).unwrap();
    let parents = (0..32).map(|_| 
        (gen_random_block_id(rng), rng.gen())
    ).collect();
    ExportActiveBlock { block, parents, is_final: rng.gen_bool(0.5), }
}

fn gen_new_stream_batch<R>(rng: &mut R) -> StreamBatch<Slot> where
    R: Rng,
{   
    let change_id = gen_random_slot(rng);
    let nb = rng.gen_range(0..100);
    let mut new_elements = BTreeMap::new();
    for _ in 0..nb {
        let key = gen_random_vector(50, rng);
        let val = gen_random_vector(150, rng);
        new_elements.insert(key, val);
    }
    let nb = rng.gen_range(0..100);
    let mut updates_on_previous_elements = BTreeMap::new();
    for _ in 0..nb {
        let key = gen_random_vector(50, rng);
        let val = if rng.gen_bool(0.5) {
            Some(gen_random_vector(150, rng))
        } else {
            None
        };
        updates_on_previous_elements.insert(key, val);
    }
    StreamBatch {
        new_elements,
        updates_on_previous_elements,
        change_id,
    }
}

fn gen_random_vector<R: Rng>(max: usize, rng: &mut R) -> Vec<u8> {
    let nb = rng.gen_range(0..max);
    let mut res = vec![];
    for _ in 0..nb {
        res.push(rng.gen());
    }
    res
}

fn stream_batch_equal<T: PartialOrd + Ord + PartialEq + Eq + Clone + std::fmt::Debug>(s1: &StreamBatch<T>, s2: &StreamBatch<T>) -> bool {
    let mut new_elements_equal = true;
    for (key1, val1) in s1.new_elements.iter() {
        if let Some(val2) = s2.new_elements.get(key1) {
            new_elements_equal &= val1 == val2;
        } else {
            new_elements_equal = false;
            break;
        }
    }
    let mut update_prevels_equal = true;
    for (key1, val1) in s1.updates_on_previous_elements.iter() {
        if let Some(val2) = s2.updates_on_previous_elements.get(key1) {
            update_prevels_equal &= val1 == val2;
        } else {
            update_prevels_equal = false;
            break;
        }
    }
    new_elements_equal && update_prevels_equal && (s1.change_id == s2.change_id)
}

#[test]
fn test_serialize_deserialize_bootstrap_msg() {
    use massa_models::config::*;
    fn perform_test<R: Rng>(rng: &mut R) {
        let config = BootstrapClientConfig {
            rate_limit: std::u64::MAX,
            max_listeners_per_peer: MAX_LISTENERS_PER_PEER as u32,
            endorsement_count: ENDORSEMENT_COUNT,
            max_advertise_length: MAX_ADVERTISE_LENGTH,
            max_bootstrap_blocks_length: MAX_BOOTSTRAP_BLOCKS,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            thread_count: THREAD_COUNT,
            randomness_size_bytes: BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
            max_bootstrap_error_length: MAX_BOOTSTRAP_ERROR_LENGTH,
            max_new_elements_size: MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE,
            max_datastore_entry_count: MAX_DATASTORE_ENTRY_COUNT,
            max_datastore_key_length: MAX_DATASTORE_KEY_LENGTH,
            max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
            max_async_pool_changes: MAX_BOOTSTRAP_ASYNC_POOL_CHANGES,
            max_async_pool_length: MAX_ASYNC_POOL_LENGTH,
            max_async_message_data: MAX_ASYNC_MESSAGE_DATA,
            max_ledger_changes_count: MAX_LEDGER_CHANGES_COUNT,
            max_changes_slot_count: 1000,
            max_rolls_length: MAX_ROLLS_COUNT_LENGTH,
            max_production_stats_length: MAX_PRODUCTION_STATS_LENGTH,
            max_credits_length: MAX_DEFERRED_CREDITS_LENGTH,
            max_executed_ops_length: MAX_EXECUTED_OPS_LENGTH,
            max_ops_changes_length: MAX_EXECUTED_OPS_CHANGES_LENGTH,
            mip_store_stats_block_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            max_denunciation_changes_length: MAX_DENUNCIATION_CHANGES_LENGTH,
        };

        let msg = BootstrapServerMessage::generate(rng);
        let mut bytes = Vec::new();
        let ser_res = BootstrapServerMessageSerializer::new().serialize(&msg, &mut bytes);
        assert!(ser_res.is_ok(), "Serialization of bootstrap server message failed");

        let deser = BootstrapServerMessageDeserializer::new((&config).into());
        match deser.deserialize::<massa_serialization::DeserializeError>(&bytes) {
            Ok((_, msg_res)) => assert!(msg_res.equals(&msg), "BootstrapServerMessages doesn't match after serialization / deserialization process"),
            Err(e) => {
                let err_str = e.to_string()[..550].to_string();
                assert!(false, "Error while deserializing: {}", err_str);
            },
        }
    }

    let regressions = vec![
        5577929984194316755,
    ];
    for reg in regressions {
        println!("[*] Regression {reg}");
        let mut rng = SmallRng::seed_from_u64(reg);
        perform_test(&mut rng);
        std::thread::sleep(Duration::from_millis(50));
    }

    let mut seeder = SmallRng::from_entropy();
    for n in 0..100_000_000 {
        let new_seed: u64 = seeder.gen();
        println!("[{n}] Seed {new_seed}");
        let mut rng = SmallRng::seed_from_u64(new_seed);
        perform_test(&mut rng);
        std::thread::sleep(Duration::from_millis(50));
    }
}
