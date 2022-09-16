// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_async_pool::{AsyncMessageId, AsyncMessageIdDeserializer, AsyncMessageIdSerializer};
use massa_final_state::{
    ExecutedOpsStreamingStep, StateChanges, StateChangesDeserializer, StateChangesSerializer,
};
use massa_graph::{
    BootstrapableGraph, BootstrapableGraphDeserializer, BootstrapableGraphSerializer,
};
use massa_ledger_exports::{KeyDeserializer, KeySerializer};
use massa_models::serialization::{VecU8Deserializer, VecU8Serializer};
use massa_models::slot::SlotDeserializer;
use massa_models::{
    slot::Slot,
    slot::SlotSerializer,
    version::{Version, VersionDeserializer, VersionSerializer},
};
use massa_network_exports::{BootstrapPeers, BootstrapPeersDeserializer, BootstrapPeersSerializer};
use massa_pos_exports::{PoSCycleStreamingStep, PoSCycleStreamingStepSerializer};
use massa_serialization::{
    Deserializer, OptionDeserializer, OptionSerializer, SerializeError, Serializer,
    U32VarIntDeserializer, U32VarIntSerializer, U64VarIntDeserializer, U64VarIntSerializer,
};
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
    /// Consensus state
    ConsensusState {
        /// block graph
        graph: BootstrapableGraph,
    },
    /// Part of the final state
    FinalStatePart {
        /// Part of the execution ledger sent in a serialized way
        ledger_data: Vec<u8>,
        /// Part of the async pool
        async_pool_part: Vec<u8>,
        /// Part of the Proof of Stake cycle_history
        pos_cycle_part: Vec<u8>,
        /// Part of the Proof of Stake deferred_credits
        pos_credits_part: Vec<u8>,
        /// Slot the state changes are attached to
        slot: Slot,
        /// Ledger change for addresses inferior to `address` of the client message until the actual slot.
        final_state_changes: Vec<(Slot, StateChanges)>,
    },
    /// Message sent when there is no state part left
    FinalStateFinished,
    /// Slot sent to get state changes is too old
    SlotTooOld,
    /// Bootstrap error
    BootstrapError {
        /// Error message
        error: String,
    },
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u32)]
enum MessageServerTypeId {
    BootstrapTime = 0u32,
    Peers = 1u32,
    ConsensusState = 2u32,
    FinalStatePart = 3u32,
    FinalStateFinished = 4u32,
    SlotTooOld = 5u32,
    BootstrapError = 6u32,
}

/// Serializer for `BootstrapServerMessage`
#[derive(Default)]
pub struct BootstrapServerMessageSerializer {
    u32_serializer: U32VarIntSerializer,
    time_serializer: MassaTimeSerializer,
    version_serializer: VersionSerializer,
    peers_serializer: BootstrapPeersSerializer,
    state_changes_serializer: StateChangesSerializer,
    bootstrapable_graph_serializer: BootstrapableGraphSerializer,
    vec_u8_serializer: VecU8Serializer,
    slot_serializer: SlotSerializer,
}

impl BootstrapServerMessageSerializer {
    /// Creates a new `BootstrapServerMessageSerializer`
    pub fn new() -> Self {
        Self {
            u32_serializer: U32VarIntSerializer::new(),
            time_serializer: MassaTimeSerializer::new(),
            version_serializer: VersionSerializer::new(),
            peers_serializer: BootstrapPeersSerializer::new(),
            state_changes_serializer: StateChangesSerializer::new(),
            bootstrapable_graph_serializer: BootstrapableGraphSerializer::new(),
            vec_u8_serializer: VecU8Serializer::new(),
            slot_serializer: SlotSerializer::new(),
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
    ///    server_time: MassaTime::from(0),
    ///    version: Version::from_str("TEST.1.0").unwrap(),
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
            BootstrapServerMessage::ConsensusState { graph } => {
                self.u32_serializer
                    .serialize(&u32::from(MessageServerTypeId::ConsensusState), buffer)?;
                self.bootstrapable_graph_serializer
                    .serialize(graph, buffer)?;
            }
            BootstrapServerMessage::FinalStatePart {
                ledger_data,
                async_pool_part,
                pos_cycle_part,
                pos_credits_part,
                slot,
                final_state_changes,
            } => {
                self.u32_serializer
                    .serialize(&u32::from(MessageServerTypeId::FinalStatePart), buffer)?;
                self.vec_u8_serializer.serialize(ledger_data, buffer)?;
                self.vec_u8_serializer.serialize(async_pool_part, buffer)?;
                self.vec_u8_serializer.serialize(pos_cycle_part, buffer)?;
                self.vec_u8_serializer.serialize(pos_credits_part, buffer)?;
                self.slot_serializer.serialize(slot, buffer)?;
                self.u32_serializer
                    .serialize(&(final_state_changes.len() as u32), buffer)?;
                for (slot, state_changes) in final_state_changes {
                    self.slot_serializer.serialize(slot, buffer)?;
                    self.state_changes_serializer
                        .serialize(state_changes, buffer)?;
                }
            }
            BootstrapServerMessage::FinalStateFinished => {
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
    length_state_changes: U32VarIntDeserializer,
    state_changes_deserializer: StateChangesDeserializer,
    bootstrapable_graph_deserializer: BootstrapableGraphDeserializer,
    final_state_parts_deserializer: VecU8Deserializer,
    length_bootstrap_error: U32VarIntDeserializer,
    slot_deserializer: SlotDeserializer,
}

impl BootstrapServerMessageDeserializer {
    /// Creates a new `BootstrapServerMessageDeserializer`
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        thread_count: u8,
        endorsement_count: u32,
        max_advertise_length: u32,
        max_bootstrap_blocks: u32,
        max_operations_per_block: u32,
        max_bootstrap_final_state_parts_size: u64,
        max_async_pool_changes: u64,
        max_data_async_message: u64,
        max_ledger_changes_count: u64,
        max_datastore_key_length: u8,
        max_datastore_value_length: u64,
        max_datastore_entry_count: u64,
        max_function_name_length: u16,
        max_parameters_size: u32,
        max_bootstrap_error_length: u32,
        max_changes_slot_count: u32,
    ) -> Self {
        Self {
            message_id_deserializer: U32VarIntDeserializer::new(Included(0), Included(u32::MAX)),
            time_deserializer: MassaTimeDeserializer::new((
                Included(MassaTime::from_millis(0)),
                Included(MassaTime::from_millis(u64::MAX)),
            )),
            version_deserializer: VersionDeserializer::new(),
            peers_deserializer: BootstrapPeersDeserializer::new(max_advertise_length),
            state_changes_deserializer: StateChangesDeserializer::new(
                thread_count,
                max_async_pool_changes,
                max_data_async_message,
                max_ledger_changes_count,
                max_datastore_key_length,
                max_datastore_value_length,
                max_datastore_entry_count,
            ),
            length_state_changes: U32VarIntDeserializer::new(
                Included(0),
                Included(max_changes_slot_count),
            ),
            bootstrapable_graph_deserializer: BootstrapableGraphDeserializer::new(
                thread_count,
                endorsement_count,
                max_bootstrap_blocks,
                max_datastore_value_length,
                max_function_name_length,
                max_parameters_size,
                max_operations_per_block,
            ),
            final_state_parts_deserializer: VecU8Deserializer::new(
                Included(0),
                Included(max_bootstrap_final_state_parts_size),
            ),
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            length_bootstrap_error: U32VarIntDeserializer::new(
                Included(0),
                Included(max_bootstrap_error_length),
            ),
        }
    }
}

impl Deserializer<BootstrapServerMessage> for BootstrapServerMessageDeserializer {
    /// ## Example
    /// ```rust
    /// use massa_bootstrap::{BootstrapServerMessage, BootstrapServerMessageSerializer, BootstrapServerMessageDeserializer};
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use massa_time::MassaTime;
    /// use massa_models::version::Version;
    /// use std::str::FromStr;
    ///
    /// let message_serializer = BootstrapServerMessageSerializer::new();
    /// let message_deserializer = BootstrapServerMessageDeserializer::new(16, 10, 100, 100, 1000, 1000, 1000, 1000, 1000, 255, 100000, 10000, 10000, 10000, 100000, 1000);
    /// let bootstrap_server_message = BootstrapServerMessage::BootstrapTime {
    ///    server_time: MassaTime::from(0),
    ///    version: Version::from_str("TEST.1.0").unwrap(),
    /// };
    /// let mut message_serialized = Vec::new();
    /// message_serializer.serialize(&bootstrap_server_message, &mut message_serialized).unwrap();
    /// let (rest, message_deserialized) = message_deserializer.deserialize::<DeserializeError>(&message_serialized).unwrap();
    /// match message_deserialized {
    ///     BootstrapServerMessage::BootstrapTime {
    ///        server_time,
    ///        version,
    ///    } => {
    ///     assert_eq!(server_time, MassaTime::from(0));
    ///     assert_eq!(version, Version::from_str("TEST.1.0").unwrap());
    ///   },
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
                MessageServerTypeId::ConsensusState => {
                    context("Failed graph deserialization", |input| {
                        self.bootstrapable_graph_deserializer.deserialize(input)
                    })
                    .map(|graph| BootstrapServerMessage::ConsensusState { graph })
                    .parse(input)
                }
                MessageServerTypeId::FinalStatePart => tuple((
                    context("Failed ledger_data deserialization", |input| {
                        self.final_state_parts_deserializer.deserialize(input)
                    }),
                    context("Failed async_pool_part deserialization", |input| {
                        self.final_state_parts_deserializer.deserialize(input)
                    }),
                    context("Failed pos_cycle_part deserialization", |input| {
                        self.final_state_parts_deserializer.deserialize(input)
                    }),
                    context("Failed pos_credits_part deserialization", |input| {
                        self.final_state_parts_deserializer.deserialize(input)
                    }),
                    context("Failed slot deserialization", |input| {
                        self.slot_deserializer.deserialize(input)
                    }),
                    context(
                        "Failed final_state_changes deserialization",
                        length_count(
                            context("Failed length deserialization", |input| {
                                self.length_state_changes.deserialize(input)
                            }),
                            tuple((
                                |input| self.slot_deserializer.deserialize(input),
                                |input| self.state_changes_deserializer.deserialize(input),
                            )),
                        ),
                    ),
                ))
                .map(
                    |(
                        ledger_data,
                        async_pool_part,
                        pos_cycle_part,
                        pos_credits_part,
                        slot,
                        final_state_changes,
                    )| {
                        BootstrapServerMessage::FinalStatePart {
                            ledger_data,
                            async_pool_part,
                            pos_cycle_part,
                            pos_credits_part,
                            slot,
                            final_state_changes,
                        }
                    },
                )
                .parse(input),
                MessageServerTypeId::FinalStateFinished => {
                    Ok((input, BootstrapServerMessage::FinalStateFinished))
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
pub enum BootstrapClientMessage {
    /// Ask for bootstrap peers
    AskBootstrapPeers,
    /// Ask for consensus state
    AskConsensusState,
    /// Ask for a part of the final state
    AskFinalStatePart {
        /// Slot we are attached to for changes
        last_slot: Option<Slot>,
        /// Last key of the ledger we received from the server
        last_key: Option<Vec<u8>>,
        /// Last async message id  of the async message pool we received from the server
        last_async_message_id: Option<AsyncMessageId>,
        /// Last received Proof of Stake cycle
        last_cycle_step: PoSCycleStreamingStep,
        /// Last receive Proof of Stake credits slot
        last_credits_slot: Option<Slot>,
        /// Last executed operations streaming step
        last_exec_ops_step: ExecutedOpsStreamingStep,
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
    AskConsensusState = 1u32,
    AskFinalStatePart = 2u32,
    BootstrapError = 3u32,
    BootstrapSuccess = 4u32,
}

/// Serializer for `BootstrapClientMessage`
pub struct BootstrapClientMessageSerializer {
    u32_serializer: U32VarIntSerializer,
    slot_serializer: SlotSerializer,
    async_message_id_serializer: AsyncMessageIdSerializer,
    key_serializer: KeySerializer,
    cycle_step_serializer: PoSCycleStreamingStepSerializer,
    opt_slot_serializer: OptionSerializer<Slot, SlotSerializer>,
}

impl BootstrapClientMessageSerializer {
    /// Creates a new `BootstrapClientMessageSerializer`
    pub fn new() -> Self {
        Self {
            u32_serializer: U32VarIntSerializer::new(),
            slot_serializer: SlotSerializer::new(),
            async_message_id_serializer: AsyncMessageIdSerializer::new(),
            key_serializer: KeySerializer::new(),
            cycle_step_serializer: PoSCycleStreamingStepSerializer::new(),
            opt_slot_serializer: OptionSerializer::new(SlotSerializer::new()),
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
            BootstrapClientMessage::AskConsensusState => {
                self.u32_serializer
                    .serialize(&u32::from(MessageClientTypeId::AskConsensusState), buffer)?;
            }
            BootstrapClientMessage::AskFinalStatePart {
                last_slot,
                last_key,
                last_async_message_id,
                last_cycle_step,
                last_credits_slot,
                last_exec_ops_step,
            } => {
                self.u32_serializer
                    .serialize(&u32::from(MessageClientTypeId::AskFinalStatePart), buffer)?;
                // If we have a cursor we must have also a slot
                if let Some(key) = last_key && let Some(slot) = last_slot && let Some(last_async_message_id) = last_async_message_id  {
                    self.key_serializer.serialize(key, buffer)?;
                    self.slot_serializer.serialize(slot, buffer)?;
                    self.async_message_id_serializer.serialize(last_async_message_id, buffer)?;
                    self.cycle_step_serializer.serialize(last_cycle_step, buffer)?;
                    self.opt_slot_serializer.serialize(last_credits_slot, buffer)?;
                    // TODO: ser last_exec_ops_step
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
    slot_deserializer: SlotDeserializer,
    async_message_id_deserializer: AsyncMessageIdDeserializer,
    length_error_deserializer: U32VarIntDeserializer,
    key_deserializer: KeyDeserializer,
    opt_u64_deserializer: OptionDeserializer<u64, U64VarIntDeserializer>,
    opt_slot_deserializer: OptionDeserializer<Slot, SlotDeserializer>,
}

impl BootstrapClientMessageDeserializer {
    /// Creates a new `BootstrapClientMessageDeserializer`
    pub fn new(thread_count: u8, max_datastore_key_length: u8) -> Self {
        Self {
            id_deserializer: U32VarIntDeserializer::new(Included(0), Included(u32::MAX)),
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            async_message_id_deserializer: AsyncMessageIdDeserializer::new(thread_count),
            key_deserializer: KeyDeserializer::new(max_datastore_key_length),
            length_error_deserializer: U32VarIntDeserializer::new(Included(0), Included(100000)),
            opt_u64_deserializer: OptionDeserializer::new(U64VarIntDeserializer::new(
                Included(0),
                Included(100000),
            )),
            opt_slot_deserializer: OptionDeserializer::new(SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            )),
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
    /// let message_deserializer = BootstrapClientMessageDeserializer::new(32, 255);
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
                MessageClientTypeId::AskConsensusState => {
                    Ok((input, BootstrapClientMessage::AskConsensusState))
                }
                MessageClientTypeId::AskFinalStatePart => {
                    if input.is_empty() {
                        Ok((
                            input,
                            BootstrapClientMessage::AskFinalStatePart {
                                last_slot: None,
                                last_key: None,
                                last_async_message_id: None,
                                last_cycle: None,
                                last_credits_slot: None,
                            },
                        ))
                    } else {
                        tuple((
                            context("Faild key deserialization", |input| {
                                self.key_deserializer.deserialize(input)
                            }),
                            context("Failed slot deserialization", |input| {
                                self.slot_deserializer.deserialize(input)
                            }),
                            context("Failed async_message_id deserialization", |input| {
                                self.async_message_id_deserializer.deserialize(input)
                            }),
                            context("Failed cycle deserialization", |input| {
                                self.opt_u64_deserializer.deserialize(input)
                            }),
                            context("Failed credits_slot deserialization", |input| {
                                self.opt_slot_deserializer.deserialize(input)
                            }),
                        ))
                        .map(
                            |(
                                last_key,
                                slot,
                                last_async_message_id,
                                last_cycle,
                                last_credits_slot,
                            )| {
                                BootstrapClientMessage::AskFinalStatePart {
                                    last_key: Some(last_key),
                                    slot: Some(slot),
                                    last_async_message_id: Some(last_async_message_id),
                                    last_cycle,
                                    last_credits_slot,
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
