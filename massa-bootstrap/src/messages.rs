// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_async_pool::{AsyncMessageId, AsyncMessageIdDeserializer, AsyncMessageIdSerializer};
use massa_final_state::{StateChanges, StateChangesDeserializer, StateChangesSerializer};
use massa_graph::BootstrapableGraph;
use massa_ledger_exports::{KeyDeserializer, KeySerializer};
use massa_models::constants::MAX_ADVERTISE_LENGTH;
use massa_models::slot::SlotDeserializer;
use massa_models::{
    constants::THREAD_COUNT, slot::SlotSerializer, DeserializeCompact, SerializeCompact, Slot,
    Version,
};
use massa_models::{VecU8Deserializer, VecU8Serializer, VersionDeserializer, VersionSerializer};
use massa_network_exports::{BootstrapPeers, BootstrapPeersDeserializer, BootstrapPeersSerializer};
use massa_proof_of_stake_exports::{
    ExportProofOfStake, ExportProofOfStakeDeserializer, ExportProofOfStakeSerializer,
};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U32VarIntDeserializer, U32VarIntSerializer,
};
use massa_time::{MassaTime, MassaTimeDeserializer, MassaTimeSerializer};
use nom::error::context;
use nom::multi::length_data;
use nom::sequence::tuple;
use nom::Parser;
use nom::{
    error::{ContextError, ParseError},
    IResult,
};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use std::convert::TryInto;
use std::ops::Bound::Included;

/// Messages used during bootstrap by server
#[derive(Debug, Clone)]
pub enum BootstrapServerMessage {
    /// Sync clocks
    BootstrapTime {
        /// The current time on the bootstrap server.
        server_time: MassaTime,
        version: Version,
    },
    /// Bootstrap peers
    BootstrapPeers {
        /// Server peers
        peers: BootstrapPeers,
    },
    /// Consensus state
    ConsensusState {
        /// PoS
        pos: ExportProofOfStake,
        /// block graph
        graph: BootstrapableGraph,
    },
    /// Part of the final state
    FinalStatePart {
        /// Part of the execution ledger sent in a serialized way
        ledger_data: Vec<u8>,
        /// Part of the async pool
        async_pool_part: Vec<u8>,
        /// Slot the state changes are attached to
        slot: Slot,
        /// Ledger change for addresses inferior to `address` of the client message until the actual slot.
        final_state_changes: StateChanges,
    },
    /// Message sent when there is no state part left
    FinalStateFinished,
    /// Slot sent to get state changes is too old
    SlotTooOld,
    /// Bootstrap error
    BootstrapError { error: String },
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
    time_serializer: MassaTimeSerializer,
    version_serializer: VersionSerializer,
    peers_serializer: BootstrapPeersSerializer,
    pos_serializer: ExportProofOfStakeSerializer,
    state_changes_serializer: StateChangesSerializer,
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
            pos_serializer: ExportProofOfStakeSerializer::new(),
            state_changes_serializer: StateChangesSerializer::new(),
            vec_u8_serializer: VecU8Serializer::new(),
            slot_serializer: SlotSerializer::new(),
        }
    }
}

impl Serializer<BootstrapServerMessage> for BootstrapServerMessageSerializer {
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
            BootstrapServerMessage::ConsensusState { pos, graph } => {
                self.u32_serializer
                    .serialize(&u32::from(MessageServerTypeId::ConsensusState), buffer)?;
                self.pos_serializer.serialize(pos, buffer)?;
                buffer.extend(graph.to_bytes_compact().map_err(|_| {
                    SerializeError::GeneralError("Fail consensus serialization".to_string())
                })?);
            }
            BootstrapServerMessage::FinalStatePart {
                ledger_data,
                async_pool_part,
                slot,
                final_state_changes,
            } => {
                self.u32_serializer
                    .serialize(&u32::from(MessageServerTypeId::FinalStatePart), buffer)?;
                self.vec_u8_serializer.serialize(ledger_data, buffer)?;
                self.vec_u8_serializer.serialize(async_pool_part, buffer)?;
                self.slot_serializer.serialize(slot, buffer)?;
                self.state_changes_serializer
                    .serialize(final_state_changes, buffer)?;
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
    u32_deserializer: U32VarIntDeserializer,
    time_deserializer: MassaTimeDeserializer,
    version_deserializer: VersionDeserializer,
    peers_deserializer: BootstrapPeersDeserializer,
    pos_deserializer: ExportProofOfStakeDeserializer,
    state_changes_deserializer: StateChangesDeserializer,
    vec_u8_deserializer: VecU8Deserializer,
    slot_deserializer: SlotDeserializer,
}

impl BootstrapServerMessageDeserializer {
    /// Creates a new `BootstrapServerMessageDeserializer`
    pub fn new() -> Self {
        Self {
            u32_deserializer: U32VarIntDeserializer::new(Included(0), Included(100)),
            time_deserializer: MassaTimeDeserializer::new((
                Included(MassaTime::from(0)),
                Included(MassaTime::from(u64::MAX)),
            )),
            version_deserializer: VersionDeserializer::new(),
            peers_deserializer: BootstrapPeersDeserializer::new(MAX_ADVERTISE_LENGTH),
            pos_deserializer: ExportProofOfStakeDeserializer::new(),
            state_changes_deserializer: StateChangesDeserializer::new(),
            vec_u8_deserializer: VecU8Deserializer::new(Included(0), Included(u64::MAX)),
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Included(THREAD_COUNT)),
            ),
        }
    }
}

impl Deserializer<BootstrapServerMessage> for BootstrapServerMessageDeserializer {
    fn deserialize<'a, E: nom::error::ParseError<&'a [u8]> + nom::error::ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BootstrapServerMessage, E> {
        context("Failed BootstrapServerMessage deserialization", |buffer| {
            let (input, id) = self.u32_deserializer.deserialize(buffer)?;
            let id = MessageServerTypeId::try_from(id).map_err(|_| {
                nom::Err::Error(ParseError::from_error_kind(
                    buffer,
                    nom::error::ErrorKind::Eof,
                ))
            })?;
            match id {
                MessageServerTypeId::BootstrapTime => tuple((
                    |input| self.time_deserializer.deserialize(input),
                    |input| self.version_deserializer.deserialize(input),
                ))
                .map(
                    |(server_time, version)| BootstrapServerMessage::BootstrapTime {
                        server_time,
                        version,
                    },
                )
                .parse(input),
                MessageServerTypeId::Peers => self
                    .peers_deserializer
                    .deserialize(input)
                    .map(|(rest, peers)| (rest, BootstrapServerMessage::BootstrapPeers { peers })),
                MessageServerTypeId::ConsensusState => tuple((
                    |input| self.pos_deserializer.deserialize(input),
                    |input| {
                        let (graph, delta) = BootstrapableGraph::from_bytes_compact(input)
                            .map_err(|_| {
                                nom::Err::Error(ParseError::from_error_kind(
                                    input,
                                    nom::error::ErrorKind::Eof,
                                ))
                            })?;
                        Ok((&input[delta..], graph))
                    },
                ))
                .map(|(pos, graph)| BootstrapServerMessage::ConsensusState { pos, graph })
                .parse(input),
                MessageServerTypeId::FinalStatePart => tuple((
                    |input| self.vec_u8_deserializer.deserialize(input),
                    |input| self.vec_u8_deserializer.deserialize(input),
                    |input| self.slot_deserializer.deserialize(input),
                    |input| self.state_changes_deserializer.deserialize(input),
                ))
                .map(
                    |(ledger_data, async_pool_part, slot, final_state_changes)| {
                        BootstrapServerMessage::FinalStatePart {
                            ledger_data,
                            async_pool_part,
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
                MessageServerTypeId::BootstrapError => {
                    length_data(|input| self.u32_deserializer.deserialize(input))
                        .map(|error| BootstrapServerMessage::BootstrapError {
                            error: String::from_utf8_lossy(error).into_owned(),
                        })
                        .parse(input)
                }
            }
        })
        .parse(buffer)
    }
}

/// Messages used during bootstrap by client
#[derive(Debug)]
pub enum BootstrapClientMessage {
    /// Ask for bootstrap peers
    AskBootstrapPeers,
    /// Ask for consensus state
    AskConsensusState,
    /// Ask for a part of the final state
    AskFinalStatePart {
        /// Last key of the ledger we received from the server
        last_key: Option<Vec<u8>>,
        /// Slot we are attached to for ledger changes
        slot: Option<Slot>,
        /// Last async message id  of the async message pool we received from the server
        last_async_message_id: Option<AsyncMessageId>,
    },
    /// Bootstrap error
    BootstrapError { error: String },
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
}

impl BootstrapClientMessageSerializer {
    /// Creates a new `BootstrapClientMessageSerializer`
    pub fn new() -> Self {
        Self {
            u32_serializer: U32VarIntSerializer::new(),
            slot_serializer: SlotSerializer::new(),
            async_message_id_serializer: AsyncMessageIdSerializer::new(),
            key_serializer: KeySerializer::new(),
        }
    }
}

impl Serializer<BootstrapClientMessage> for BootstrapClientMessageSerializer {
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
                last_key,
                slot,
                last_async_message_id,
            } => {
                self.u32_serializer
                    .serialize(&u32::from(MessageClientTypeId::AskFinalStatePart), buffer)?;
                // If we have a cursor we must have also a slot
                if let Some(key) = last_key && let Some(slot) = slot && let Some(last_async_message_id) = last_async_message_id  {
                    self.key_serializer.serialize(key, buffer)?;
                    self.slot_serializer.serialize(slot, buffer)?;
                    self.async_message_id_serializer.serialize(last_async_message_id, buffer)?;
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
    u32_deserializer: U32VarIntDeserializer,
    slot_deserializer: SlotDeserializer,
    async_message_id_deserializer: AsyncMessageIdDeserializer,
    key_deserializer: KeyDeserializer,
}

impl BootstrapClientMessageDeserializer {
    /// Creates a new `BootstrapClientMessageDeserializer`
    pub fn new() -> Self {
        Self {
            u32_deserializer: U32VarIntDeserializer::new(Included(0), Included(1000)),
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Included(THREAD_COUNT)),
            ),
            async_message_id_deserializer: AsyncMessageIdDeserializer::new(),
            key_deserializer: KeyDeserializer::new(),
        }
    }
}

impl Deserializer<BootstrapClientMessage> for BootstrapClientMessageDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BootstrapClientMessage, E> {
        context("Failed BootstrapClientMessage deserialization", |buffer| {
            let (input, id) = self.u32_deserializer.deserialize(buffer)?;
            let id = MessageClientTypeId::try_from(id).map_err(|_| {
                nom::Err::Error(ParseError::from_error_kind(
                    buffer,
                    nom::error::ErrorKind::Eof,
                ))
            })?;
            match id {
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
                                last_key: None,
                                slot: None,
                                last_async_message_id: None,
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
                        ))
                        .map(|(last_key, slot, last_async_message_id)| {
                            BootstrapClientMessage::AskFinalStatePart {
                                last_key: Some(last_key),
                                slot: Some(slot),
                                last_async_message_id: Some(last_async_message_id),
                            }
                        })
                        .parse(input)
                    }
                }
                MessageClientTypeId::BootstrapError => {
                    length_data(|input| self.u32_deserializer.deserialize(input))
                        .map(|error| BootstrapClientMessage::BootstrapError {
                            error: String::from_utf8_lossy(error).into_owned(),
                        })
                        .parse(input)
                }
                MessageClientTypeId::BootstrapSuccess => {
                    Ok((input, BootstrapClientMessage::BootstrapSuccess))
                }
            }
        })
        .parse(buffer)
    }
}
