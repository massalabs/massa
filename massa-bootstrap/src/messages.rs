// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::settings::BootstrapServerMessageDeserializerArgs;
use massa_consensus_exports::bootstrapable_graph::{
    BootstrapableGraph, BootstrapableGraphDeserializer, BootstrapableGraphSerializer,
};

use massa_db_exports::StreamBatch;

use massa_models::block_id::{BlockId, BlockIdDeserializer, BlockIdSerializer};

use massa_models::prehash::PreHashSet;
use massa_models::serialization::{
    PreHashSetDeserializer, PreHashSetSerializer, VecU8Deserializer, VecU8Serializer,
};
use massa_models::slot::{Slot, SlotDeserializer, SlotSerializer};
use massa_models::streaming_step::{
    StreamingStep, StreamingStepDeserializer, StreamingStepSerializer,
};
use massa_models::version::{Version, VersionDeserializer, VersionSerializer};
use massa_protocol_exports::{
    BootstrapPeers, BootstrapPeersDeserializer, BootstrapPeersSerializer,
};
use massa_serialization::{
    BoolDeserializer, BoolSerializer, Deserializer, OptionDeserializer, OptionSerializer,
    SerializeError, Serializer, U32VarIntDeserializer, U32VarIntSerializer, U64VarIntDeserializer,
    U64VarIntSerializer,
};

use massa_time::{MassaTime, MassaTimeDeserializer, MassaTimeSerializer};
use nom::error::context;
use nom::multi::{length_data, length_value, many0};
use nom::sequence::tuple;
use nom::Parser;
use nom::{
    error::{ContextError, ParseError},
    IResult,
};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use std::convert::TryInto;
use std::ops::Bound::{Excluded, Included};

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

#[allow(clippy::to_string_trait_impl)]
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
                // state new_elements
                let mut state_new_element_buffer: Vec<u8> = Vec::new();
                for (key, value) in state_part.new_elements.iter() {
                    self.vec_u8_serializer
                        .serialize(key, &mut state_new_element_buffer)?;
                    self.vec_u8_serializer
                        .serialize(value, &mut state_new_element_buffer)?;
                }
                self.u64_serializer.serialize(
                    &state_new_element_buffer
                        .len()
                        .try_into()
                        .expect("Overflow of state new_elements len"),
                    buffer,
                )?;
                buffer.extend(state_new_element_buffer);
                // state updates
                let mut state_updates_buffer: Vec<u8> = Vec::new();
                for (key, value) in state_part.updates_on_previous_elements.iter() {
                    self.vec_u8_serializer
                        .serialize(key, &mut state_updates_buffer)?;
                    self.opt_vec_u8_serializer
                        .serialize(value, &mut state_updates_buffer)?;
                }
                self.u64_serializer.serialize(
                    &state_updates_buffer
                        .len()
                        .try_into()
                        .expect("Overflow of state updates len"),
                    buffer,
                )?;
                buffer.extend(state_updates_buffer);
                self.slot_serializer
                    .serialize(&state_part.change_id, buffer)?;
                // versioning new_elements
                let mut versioning_new_element_buffer: Vec<u8> = Vec::new();
                for (key, value) in versioning_part.new_elements.iter() {
                    self.vec_u8_serializer
                        .serialize(key, &mut versioning_new_element_buffer)?;
                    self.vec_u8_serializer
                        .serialize(value, &mut versioning_new_element_buffer)?;
                }
                self.u64_serializer.serialize(
                    &versioning_new_element_buffer
                        .len()
                        .try_into()
                        .expect("Overflow of versioning new_elements len"),
                    buffer,
                )?;
                buffer.extend(versioning_new_element_buffer);
                // versioning updates
                let mut versioning_updates_buffer: Vec<u8> = Vec::new();
                for (key, value) in versioning_part.updates_on_previous_elements.iter() {
                    self.vec_u8_serializer
                        .serialize(key, &mut versioning_updates_buffer)?;
                    self.opt_vec_u8_serializer
                        .serialize(value, &mut versioning_updates_buffer)?;
                }
                self.u64_serializer.serialize(
                    &versioning_updates_buffer
                        .len()
                        .try_into()
                        .expect("Overflow of versioning updates len"),
                    buffer,
                )?;
                buffer.extend(versioning_updates_buffer);
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
    versioning_part_new_elements_length_deserializer: U64VarIntDeserializer,
    stream_batch_updates_length_deserializer: U64VarIntDeserializer,
    datastore_key_deserializer: VecU8Deserializer,
    datastore_val_deserializer: VecU8Deserializer,
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
            datastore_key_deserializer: VecU8Deserializer::new(
                Included(0),
                Included(args.max_datastore_key_length.into()),
            ),
            datastore_val_deserializer: VecU8Deserializer::new(
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
                Included(args.max_bootstrap_blocks_length.into()),
            ),
            length_bootstrap_error: U64VarIntDeserializer::new(
                Included(0),
                Included(args.max_bootstrap_error_length),
            ),
            versioning_part_new_elements_length_deserializer: U64VarIntDeserializer::new(
                Included(0),
                Included(args.max_versioning_elements_size.into()),
            ),
            state_new_elements_length_deserializer: U64VarIntDeserializer::new(
                Included(0),
                Included(args.max_final_state_elements_size.into()),
            ),
            stream_batch_updates_length_deserializer: U64VarIntDeserializer::new(
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
    /// use massa_models::config::CHAINID;
    ///
    /// let message_serializer = BootstrapServerMessageSerializer::new();
    /// let args = BootstrapServerMessageDeserializerArgs {
    ///     thread_count: 32, endorsement_count: 16,
    ///     max_listeners_per_peer: 1000,
    ///     max_advertise_length: 1000, max_bootstrap_blocks_length: 1000,
    ///     max_operations_per_block: 1000, max_versioning_elements_size: 1000,
    ///     max_ledger_changes_count: 1000, max_datastore_key_length: 255,
    ///     max_datastore_value_length: 1000,
    ///     max_final_state_elements_size: 1000,
    ///     max_datastore_entry_count: 1000, max_bootstrap_error_length: 1000, max_changes_slot_count: 1000,
    ///     max_rolls_length: 1000, max_production_stats_length: 1000, max_credits_length: 1000,
    ///     max_executed_ops_length: 1000, max_ops_changes_length: 1000,
    ///     mip_store_stats_block_considered: 100,
    ///     max_denunciations_per_block_header: 128, max_denunciation_changes_length: 1000,
    ///     chain_id: *CHAINID
    /// };
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
                                length_value(
                                    context("Failed length deserialization", |input| {
                                        self.state_new_elements_length_deserializer
                                            .deserialize(input)
                                    }),
                                    many0(tuple((
                                        |input| self.datastore_key_deserializer.deserialize(input),
                                        |input| self.datastore_val_deserializer.deserialize(input),
                                    ))),
                                ),
                            ),
                            context(
                                "Failed updates deserialization",
                                length_value(
                                    context("Failed length deserialization", |input| {
                                        self.stream_batch_updates_length_deserializer
                                            .deserialize(input)
                                    }),
                                    many0(tuple((
                                        |input| self.datastore_key_deserializer.deserialize(input),
                                        |input| self.opt_vec_u8_deserializer.deserialize(input),
                                    ))),
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
                                length_value(
                                    context("Failed length deserialization", |input| {
                                        self.versioning_part_new_elements_length_deserializer
                                            .deserialize(input)
                                    }),
                                    many0(tuple((
                                        |input| self.datastore_key_deserializer.deserialize(input),
                                        |input| self.datastore_val_deserializer.deserialize(input),
                                    ))),
                                ),
                            ),
                            context(
                                "Failed updates deserialization",
                                length_value(
                                    context("Failed length deserialization", |input| {
                                        self.stream_batch_updates_length_deserializer
                                            .deserialize(input)
                                    }),
                                    many0(tuple((
                                        |input| self.datastore_key_deserializer.deserialize(input),
                                        |input| self.opt_vec_u8_deserializer.deserialize(input),
                                    ))),
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
        max_datastore_key_length: u8,
        max_consensus_block_ids: u64,
    ) -> Self {
        Self {
            id_deserializer: U32VarIntDeserializer::new(Included(0), Included(u32::MAX)),
            length_error_deserializer: U32VarIntDeserializer::new(Included(0), Included(u32::MAX)),
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            state_step_deserializer: StreamingStepDeserializer::new(VecU8Deserializer::new(
                Included(0),
                Included(max_datastore_key_length.into()),
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
                            context("Failed last_state_step deserialization", |input| {
                                self.state_step_deserializer.deserialize(input)
                            }),
                            context("Failed last_versioning_step deserialization", |input| {
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
