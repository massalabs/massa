// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_async_pool::{
    AsyncMessage, AsyncMessageId, AsyncMessageIdDeserializer, AsyncMessageIdSerializer,
    AsyncPoolDeserializer, AsyncPoolSerializer,
};
use massa_final_state::{
    ExecutedOps, ExecutedOpsDeserializer, ExecutedOpsSerializer, StateChanges,
    StateChangesDeserializer, StateChangesSerializer,
};
use massa_graph::{
    BootstrapableGraph, BootstrapableGraphDeserializer, BootstrapableGraphSerializer,
};
use massa_ledger_exports::{KeyDeserializer, KeySerializer};
use massa_models::operation::{OperationId, OperationIdDeserializer, OperationIdSerializer};
use massa_models::serialization::{VecU8Deserializer, VecU8Serializer};
use massa_models::slot::SlotDeserializer;
use massa_models::streaming_step::{
    StreamingStep, StreamingStepDeserializer, StreamingStepSerializer,
};
use massa_models::{
    slot::Slot,
    slot::SlotSerializer,
    version::{Version, VersionDeserializer, VersionSerializer},
};
use massa_network_exports::{BootstrapPeers, BootstrapPeersDeserializer, BootstrapPeersSerializer};
use massa_pos_exports::{
    CycleInfo, CycleInfoDeserializer, CycleInfoSerializer, DeferredCredits,
    DeferredCreditsDeserializer, DeferredCreditsSerializer,
};
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
use std::collections::BTreeMap;
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
        /// Slot the state changes are attached to
        slot: Slot,
        /// Part of the execution ledger sent in a serialized way
        ledger_part: Vec<u8>,
        /// Part of the async pool
        async_pool_part: BTreeMap<AsyncMessageId, AsyncMessage>,
        /// Part of the Proof of Stake `cycle_history`
        pos_cycle_part: Option<CycleInfo>,
        /// Part of the Proof of Stake `deferred_credits`
        pos_credits_part: DeferredCredits,
        /// Part of the executed operations
        exec_ops_part: ExecutedOps,
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
pub struct BootstrapServerMessageSerializer {
    u32_serializer: U32VarIntSerializer,
    u64_serializer: U64VarIntSerializer,
    time_serializer: MassaTimeSerializer,
    version_serializer: VersionSerializer,
    peers_serializer: BootstrapPeersSerializer,
    state_changes_serializer: StateChangesSerializer,
    bootstrapable_graph_serializer: BootstrapableGraphSerializer,
    vec_u8_serializer: VecU8Serializer,
    slot_serializer: SlotSerializer,
    async_pool_serializer: AsyncPoolSerializer,
    opt_pos_cycle_serializer: OptionSerializer<CycleInfo, CycleInfoSerializer>,
    pos_credits_serializer: DeferredCreditsSerializer,
    exec_ops_serializer: ExecutedOpsSerializer,
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
            state_changes_serializer: StateChangesSerializer::new(),
            bootstrapable_graph_serializer: BootstrapableGraphSerializer::new(),
            vec_u8_serializer: VecU8Serializer::new(),
            slot_serializer: SlotSerializer::new(),
            async_pool_serializer: AsyncPoolSerializer::new(),
            opt_pos_cycle_serializer: OptionSerializer::new(CycleInfoSerializer::new()),
            pos_credits_serializer: DeferredCreditsSerializer::new(),
            exec_ops_serializer: ExecutedOpsSerializer::new(),
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
            BootstrapServerMessage::ConsensusState { graph } => {
                self.u32_serializer
                    .serialize(&u32::from(MessageServerTypeId::ConsensusState), buffer)?;
                self.bootstrapable_graph_serializer
                    .serialize(graph, buffer)?;
            }
            BootstrapServerMessage::FinalStatePart {
                slot,
                ledger_part,
                async_pool_part,
                pos_cycle_part,
                pos_credits_part,
                exec_ops_part,
                final_state_changes,
            } => {
                // message type
                self.u32_serializer
                    .serialize(&u32::from(MessageServerTypeId::FinalStatePart), buffer)?;
                // slot
                self.slot_serializer.serialize(slot, buffer)?;
                // ledger
                self.vec_u8_serializer.serialize(ledger_part, buffer)?;
                // async pool
                self.async_pool_serializer
                    .serialize(async_pool_part, buffer)?;
                // pos cycle info
                self.opt_pos_cycle_serializer
                    .serialize(pos_cycle_part, buffer)?;
                // pos deferred credits
                self.pos_credits_serializer
                    .serialize(pos_credits_part, buffer)?;
                // executed operations
                self.exec_ops_serializer.serialize(exec_ops_part, buffer)?;
                // changes length
                self.u64_serializer
                    .serialize(&(final_state_changes.len() as u64), buffer)?;
                // changes
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
    length_state_changes: U64VarIntDeserializer,
    state_changes_deserializer: StateChangesDeserializer,
    bootstrapable_graph_deserializer: BootstrapableGraphDeserializer,
    ledger_bytes_deserializer: VecU8Deserializer,
    length_bootstrap_error: U64VarIntDeserializer,
    slot_deserializer: SlotDeserializer,
    async_pool_deserializer: AsyncPoolDeserializer,
    opt_pos_cycle_deserializer: OptionDeserializer<CycleInfo, CycleInfoDeserializer>,
    pos_credits_deserializer: DeferredCreditsDeserializer,
    exec_ops_deserializer: ExecutedOpsDeserializer,
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
        max_async_pool_length: u64,
        max_async_message_data: u64,
        max_ledger_changes_count: u64,
        max_datastore_key_length: u8,
        max_datastore_value_length: u64,
        max_datastore_entry_count: u64,
        max_function_name_length: u16,
        max_parameters_size: u32,
        max_bootstrap_error_length: u64,
        max_op_datastore_entry_count: u64,
        max_op_datastore_key_length: u8,
        max_op_datastore_value_length: u64,
        max_changes_slot_count: u64,
        max_rolls_length: u64,
        max_production_stats_length: u64,
        max_credits_length: u64,
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
                max_async_message_data,
                max_ledger_changes_count,
                max_datastore_key_length,
                max_datastore_value_length,
                max_datastore_entry_count,
                max_rolls_length,
                max_production_stats_length,
                max_credits_length,
            ),
            length_state_changes: U64VarIntDeserializer::new(
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
                max_op_datastore_entry_count,
                max_op_datastore_key_length,
                max_op_datastore_value_length,
            ),
            ledger_bytes_deserializer: VecU8Deserializer::new(
                Included(0),
                Included(max_bootstrap_final_state_parts_size),
            ),
            length_bootstrap_error: U64VarIntDeserializer::new(
                Included(0),
                Included(max_bootstrap_error_length),
            ),
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            async_pool_deserializer: AsyncPoolDeserializer::new(
                thread_count,
                max_async_pool_length,
                max_async_message_data,
            ),
            opt_pos_cycle_deserializer: OptionDeserializer::new(CycleInfoDeserializer::new(
                max_rolls_length,
                max_production_stats_length,
            )),
            pos_credits_deserializer: DeferredCreditsDeserializer::new(
                thread_count,
                max_credits_length,
            ),
            exec_ops_deserializer: ExecutedOpsDeserializer::new(thread_count),
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
    /// let message_deserializer = BootstrapServerMessageDeserializer::new(32, 16, 1000, 1000, 1000, 1000, 1000, 1000, 1000, 1000, 255, 1000, 1000, 1000, 1000, 1000, 10, 255, 1000, 1000, 10_000, 10_000, 10_000);
    /// let bootstrap_server_message = BootstrapServerMessage::BootstrapTime {
    ///    server_time: MassaTime::from(0),
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
    ///     assert_eq!(server_time, MassaTime::from(0));
    ///     assert_eq!(version, Version::from_str("TEST.1.10").unwrap());
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
                    context("Failed slot deserialization", |input| {
                        self.slot_deserializer.deserialize(input)
                    }),
                    context("Failed ledger_data deserialization", |input| {
                        self.ledger_bytes_deserializer.deserialize(input)
                    }),
                    context("Failed async_pool_part deserialization", |input| {
                        self.async_pool_deserializer.deserialize(input)
                    }),
                    context("Failed pos_cycle_part deserialization", |input| {
                        self.opt_pos_cycle_deserializer.deserialize(input)
                    }),
                    context("Failed pos_credits_part deserialization", |input| {
                        self.pos_credits_deserializer.deserialize(input)
                    }),
                    context("Failed exec_ops_part deserialization", |input| {
                        self.exec_ops_deserializer.deserialize(input)
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
                        slot,
                        ledger_part,
                        async_pool_part,
                        pos_cycle_part,
                        pos_credits_part,
                        exec_ops_part,
                        final_state_changes,
                    )| {
                        BootstrapServerMessage::FinalStatePart {
                            slot,
                            ledger_part,
                            async_pool_part,
                            pos_cycle_part,
                            pos_credits_part,
                            exec_ops_part,
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
        /// Last received ledger key
        last_ledger_step: StreamingStep<Vec<u8>>,
        /// Last received async message id
        last_pool_step: StreamingStep<AsyncMessageId>,
        /// Last received Proof of Stake cycle
        last_cycle_step: StreamingStep<u64>,
        /// Last received Proof of Stake credits slot
        last_credits_step: StreamingStep<Slot>,
        /// Last received executed operation id
        last_ops_step: StreamingStep<OperationId>,
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
    ledger_step_serializer: StreamingStepSerializer<Vec<u8>, KeySerializer>,
    pool_step_serializer: StreamingStepSerializer<AsyncMessageId, AsyncMessageIdSerializer>,
    cycle_step_serializer: StreamingStepSerializer<u64, U64VarIntSerializer>,
    credits_step_serializer: StreamingStepSerializer<Slot, SlotSerializer>,
    ops_step_serializer: StreamingStepSerializer<OperationId, OperationIdSerializer>,
}

impl BootstrapClientMessageSerializer {
    /// Creates a new `BootstrapClientMessageSerializer`
    pub fn new() -> Self {
        Self {
            u32_serializer: U32VarIntSerializer::new(),
            slot_serializer: SlotSerializer::new(),
            ledger_step_serializer: StreamingStepSerializer::new(KeySerializer::new()),
            pool_step_serializer: StreamingStepSerializer::new(AsyncMessageIdSerializer::new()),
            cycle_step_serializer: StreamingStepSerializer::new(U64VarIntSerializer::new()),
            credits_step_serializer: StreamingStepSerializer::new(SlotSerializer::new()),
            ops_step_serializer: StreamingStepSerializer::new(OperationIdSerializer::new()),
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
                last_ledger_step,
                last_pool_step,
                last_cycle_step,
                last_credits_step,
                last_ops_step,
            } => {
                self.u32_serializer
                    .serialize(&u32::from(MessageClientTypeId::AskFinalStatePart), buffer)?;
                if let Some(slot) = last_slot {
                    self.slot_serializer.serialize(slot, buffer)?;
                    self.ledger_step_serializer
                        .serialize(last_ledger_step, buffer)?;
                    self.pool_step_serializer
                        .serialize(last_pool_step, buffer)?;
                    self.cycle_step_serializer
                        .serialize(last_cycle_step, buffer)?;
                    self.credits_step_serializer
                        .serialize(last_credits_step, buffer)?;
                    self.ops_step_serializer.serialize(last_ops_step, buffer)?;
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
    ledger_step_serializer: StreamingStepDeserializer<Vec<u8>, KeyDeserializer>,
    pool_step_serializer: StreamingStepDeserializer<AsyncMessageId, AsyncMessageIdDeserializer>,
    cycle_step_serializer: StreamingStepDeserializer<u64, U64VarIntDeserializer>,
    credits_step_serializer: StreamingStepDeserializer<Slot, SlotDeserializer>,
    ops_step_serializer: StreamingStepDeserializer<OperationId, OperationIdDeserializer>,
}

impl BootstrapClientMessageDeserializer {
    /// Creates a new `BootstrapClientMessageDeserializer`
    pub fn new(thread_count: u8, max_datastore_key_length: u8) -> Self {
        Self {
            id_deserializer: U32VarIntDeserializer::new(Included(0), Included(u32::MAX)),
            length_error_deserializer: U32VarIntDeserializer::new(Included(0), Included(100000)),
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            ledger_step_serializer: StreamingStepDeserializer::new(KeyDeserializer::new(
                max_datastore_key_length,
            )),
            pool_step_serializer: StreamingStepDeserializer::new(AsyncMessageIdDeserializer::new(
                thread_count,
            )),
            cycle_step_serializer: StreamingStepDeserializer::new(U64VarIntDeserializer::new(
                Included(0),
                Included(u64::MAX),
            )),
            credits_step_serializer: StreamingStepDeserializer::new(SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            )),
            ops_step_serializer: StreamingStepDeserializer::new(OperationIdDeserializer::new()),
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
                                last_ledger_step: StreamingStep::Started,
                                last_pool_step: StreamingStep::Started,
                                last_cycle_step: StreamingStep::Started,
                                last_credits_step: StreamingStep::Started,
                                last_ops_step: StreamingStep::Started,
                            },
                        ))
                    } else {
                        tuple((
                            context("Failed last_slot deserialization", |input| {
                                self.slot_deserializer.deserialize(input)
                            }),
                            context("Faild last_ledger_step deserialization", |input| {
                                self.ledger_step_serializer.deserialize(input)
                            }),
                            context("Failed last_pool_step deserialization", |input| {
                                self.pool_step_serializer.deserialize(input)
                            }),
                            context("Failed last_cycle_step deserialization", |input| {
                                self.cycle_step_serializer.deserialize(input)
                            }),
                            context("Failed last_credits_step deserialization", |input| {
                                self.credits_step_serializer.deserialize(input)
                            }),
                            context("Failed last_ops_step deserialization", |input| {
                                self.ops_step_serializer.deserialize(input)
                            }),
                        ))
                        .map(
                            |(
                                last_slot,
                                last_ledger_step,
                                last_pool_step,
                                last_cycle_step,
                                last_credits_step,
                                last_ops_step,
                            )| {
                                BootstrapClientMessage::AskFinalStatePart {
                                    last_slot: Some(last_slot),
                                    last_ledger_step,
                                    last_pool_step,
                                    last_cycle_step,
                                    last_credits_step,
                                    last_ops_step,
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
