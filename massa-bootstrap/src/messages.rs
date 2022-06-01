// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_async_pool::{
    AsyncMessageId, AsyncMessageIdDeserializer, AsyncMessageIdSerializer, AsyncPoolPart,
    AsyncPoolPartDeserializer, AsyncPoolPartSerializer,
};
use massa_final_state::{StateChanges, StateChangesDeserializer, StateChangesSerializer};
use massa_graph::BootstrapableGraph;
use massa_ledger::{KeyDeserializer, KeySerializer};
use massa_models::slot::SlotDeserializer;
use massa_models::{
    constants::THREAD_COUNT, slot::SlotSerializer, DeserializeCompact, DeserializeVarInt,
    ModelsError, SerializeCompact, SerializeVarInt, Slot, Version,
};
use massa_models::{
    U64VarIntDeserializer, U64VarIntSerializer, VecU8Deserializer, VecU8Serializer,
};
use massa_network_exports::BootstrapPeers;
use massa_proof_of_stake_exports::ExportProofOfStake;
use massa_serialization::{Deserializer, Serializer};
use massa_time::MassaTime;
use nom::error::context;
use nom::multi::length_count;
use nom::sequence::tuple;
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
    /// Part of the ledger of execution
    FinalStatePart {
        /// Part of the execution ledger sent in a serialized way
        ledger_data: Vec<u8>,
        /// Part of the async pool
        async_pool_part: AsyncPoolPart,
        /// Slot the ledger changes are attached to
        slot: Slot,
        /// Ledger change for addresses inferior to `address` of the client message until the actual slot.
        final_state_changes: Vec<StateChanges>,
    },
    FinalStateFinished,
    /// Bootstrap error
    BootstrapError {
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
    BootstrapError = 5u32,
}

impl SerializeCompact for BootstrapServerMessage {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();
        match self {
            BootstrapServerMessage::BootstrapTime {
                server_time,
                version,
            } => {
                res.extend(u32::from(MessageServerTypeId::BootstrapTime).to_varint_bytes());
                res.extend(server_time.to_bytes_compact()?);
                res.extend(&version.to_bytes_compact()?)
            }
            BootstrapServerMessage::BootstrapPeers { peers } => {
                res.extend(u32::from(MessageServerTypeId::Peers).to_varint_bytes());
                res.extend(&peers.to_bytes_compact()?);
            }
            BootstrapServerMessage::ConsensusState { pos, graph } => {
                res.extend(u32::from(MessageServerTypeId::ConsensusState).to_varint_bytes());
                res.extend(&pos.to_bytes_compact()?);
                res.extend(&graph.to_bytes_compact()?);
            }
            BootstrapServerMessage::FinalStatePart {
                ledger_data,
                async_pool_part,
                slot,
                final_state_changes,
            } => {
                let async_pool_serializer = AsyncPoolPartSerializer::new();
                let slot_serializer = SlotSerializer::new(
                    (Included(0), Included(u64::MAX)),
                    (Included(0), Included(THREAD_COUNT)),
                );
                let final_state_changes_serializer = StateChangesSerializer::new();
                let vec_u8_serializer =
                    VecU8Serializer::new(Included(u64::MIN), Included(u64::MAX));
                let u64_serializer =
                    U64VarIntSerializer::new(Included(u64::MIN), Included(u64::MAX));
                res.extend(u32::from(MessageServerTypeId::FinalStatePart).to_varint_bytes());
                res.extend(vec_u8_serializer.serialize(ledger_data)?);
                res.extend(async_pool_serializer.serialize(async_pool_part)?);
                res.extend(slot_serializer.serialize(slot)?);
                res.extend(u64_serializer.serialize(
                    &(final_state_changes.len().try_into().map_err(|_| {
                        ModelsError::SerializeError("Fail to convert usize to u64".to_string())
                    })?),
                )?);
                for changes in final_state_changes {
                    res.extend(final_state_changes_serializer.serialize(changes)?);
                }
            }
            BootstrapServerMessage::FinalStateFinished => {
                res.extend(u32::from(MessageServerTypeId::FinalStateFinished).to_varint_bytes());
            }
            BootstrapServerMessage::BootstrapError { error } => {
                res.extend(u32::from(MessageServerTypeId::BootstrapError).to_varint_bytes());
                res.extend(u32::to_varint_bytes(error.len().try_into().map_err(
                    |_| ModelsError::SerializeError("Fail to convert usize to u32".to_string()),
                )?));
                res.extend(error.as_bytes())
            }
        }
        Ok(res)
    }
}

impl DeserializeCompact for BootstrapServerMessage {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0usize;

        let (type_id_raw, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        let type_id: MessageServerTypeId = type_id_raw
            .try_into()
            .map_err(|_| ModelsError::DeserializeError("invalid message type ID".into()))?;

        let res = match type_id {
            MessageServerTypeId::BootstrapTime => {
                let (server_time, delta) = MassaTime::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                let (version, delta) = Version::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;
                BootstrapServerMessage::BootstrapTime {
                    server_time,
                    version,
                }
            }
            MessageServerTypeId::Peers => {
                let (peers, delta) = BootstrapPeers::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                BootstrapServerMessage::BootstrapPeers { peers }
            }
            MessageServerTypeId::ConsensusState => {
                let (pos, delta) = ExportProofOfStake::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;
                let (graph, delta) = BootstrapableGraph::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                BootstrapServerMessage::ConsensusState { pos, graph }
            }
            MessageServerTypeId::FinalStatePart => {
                let async_pool_deserializer = AsyncPoolPartDeserializer::new();
                let slot_deserializer = SlotDeserializer::new(
                    (Included(0), Included(u64::MAX)),
                    (Included(0), Included(THREAD_COUNT)),
                );
                let final_state_changes_deserializer = StateChangesDeserializer::new();
                let vec_u8_deserializer =
                    VecU8Deserializer::new(Included(u64::MIN), Included(u64::MAX));
                let u64_deserializer =
                    U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX));
                let (rest, (ledger_data, async_pool_part, slot, final_state_changes)) =
                    tuple((
                        context("ledger data in final state part", |input| {
                            vec_u8_deserializer.deserialize(input)
                        }),
                        context("async pool in final state part", |input| {
                            async_pool_deserializer.deserialize(input)
                        }),
                        context("slot in final state part", |input| {
                            slot_deserializer.deserialize(input)
                        }),
                        context("changes in final state part", |input| {
                            length_count(
                                |input| u64_deserializer.deserialize(input),
                                |input| final_state_changes_deserializer.deserialize(input),
                            )(input)
                        }),
                    ))(&buffer[cursor..])?;
                // Temp while serializecompact exists
                let delta = buffer[cursor..].len() - rest.len();
                cursor += delta;
                BootstrapServerMessage::FinalStatePart {
                    ledger_data,
                    async_pool_part,
                    slot,
                    final_state_changes,
                }
            }
            MessageServerTypeId::FinalStateFinished => BootstrapServerMessage::FinalStateFinished,
            MessageServerTypeId::BootstrapError => {
                let (error_len, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
                cursor += delta;

                let error = String::from_utf8_lossy(&buffer[cursor..cursor + error_len as usize]);
                cursor += error_len as usize;

                BootstrapServerMessage::BootstrapError {
                    error: error.into_owned(),
                }
            }
        };
        Ok((res, cursor))
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
        /// Last position of the cursor received from the server
        last_key: Option<Vec<u8>>,
        /// Slot we are attached to for ledger changes
        slot: Option<Slot>,
        /// Last async message id sent
        last_async_message_id: Option<AsyncMessageId>,
    },
    /// Bootstrap error
    BootstrapError { error: String },
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u32)]
enum MessageClientTypeId {
    AskBootstrapPeers = 0u32,
    AskConsensusState = 1u32,
    AskFinalStatePart = 2u32,
    BootstrapError = 3u32,
}

impl SerializeCompact for BootstrapClientMessage {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();
        match self {
            BootstrapClientMessage::AskBootstrapPeers => {
                res.extend(u32::from(MessageClientTypeId::AskBootstrapPeers).to_varint_bytes());
            }
            BootstrapClientMessage::AskConsensusState => {
                res.extend(u32::from(MessageClientTypeId::AskConsensusState).to_varint_bytes());
            }
            BootstrapClientMessage::AskFinalStatePart {
                last_key,
                slot,
                last_async_message_id,
            } => {
                res.extend(u32::from(MessageClientTypeId::AskFinalStatePart).to_varint_bytes());
                // If we have a cursor we must have also a slot
                if let Some(key) = last_key && let Some(slot) = slot && let Some(last_async_message_id) = last_async_message_id  {
                    let async_message_id_serializer = AsyncMessageIdSerializer::new();
                    let key_serializer = KeySerializer::new();
                    res.extend(key_serializer.serialize(key)?);
                    res.extend(slot.to_bytes_compact()?);
                    res.extend(async_message_id_serializer.serialize(last_async_message_id)?);
                }
            }
            BootstrapClientMessage::BootstrapError { error } => {
                res.extend(u32::from(MessageClientTypeId::BootstrapError).to_varint_bytes());
                res.extend(u32::to_varint_bytes(error.len() as u32));
                res.extend(error.as_bytes())
            }
        }
        Ok(res)
    }
}

impl DeserializeCompact for BootstrapClientMessage {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0usize;

        let (type_id_raw, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        let type_id: MessageClientTypeId = type_id_raw
            .try_into()
            .map_err(|_| ModelsError::DeserializeError("invalid message type ID".into()))?;

        let res = match type_id {
            MessageClientTypeId::AskBootstrapPeers => BootstrapClientMessage::AskBootstrapPeers,
            MessageClientTypeId::AskConsensusState => BootstrapClientMessage::AskConsensusState,
            MessageClientTypeId::AskFinalStatePart => {
                if buffer.len() == cursor {
                    BootstrapClientMessage::AskFinalStatePart {
                        last_key: None,
                        slot: None,
                        last_async_message_id: None,
                    }
                } else {
                    let key_deserializer = KeyDeserializer::new();
                    let slot_deserializer = SlotDeserializer::new(
                        (Included(0), Included(u64::MAX)),
                        (Included(0), Included(THREAD_COUNT)),
                    );
                    let async_message_id_deserializer = AsyncMessageIdDeserializer::new();
                    let (rest, (last_key, slot, last_async_message_id)) =
                        tuple((
                            context("last_key in ask final state part", |input| {
                                key_deserializer.deserialize(input)
                            }),
                            context("slot in ask final state part", |input| {
                                slot_deserializer.deserialize(input)
                            }),
                            context("async message id in final state part", |input| {
                                async_message_id_deserializer.deserialize(input)
                            }),
                        ))(&buffer[cursor..])?;
                    // Temp while serializecompact exists
                    let delta = buffer[cursor..].len() - rest.len();
                    cursor += delta;

                    BootstrapClientMessage::AskFinalStatePart {
                        last_key: Some(last_key),
                        slot: Some(slot),
                        last_async_message_id: Some(last_async_message_id),
                    }
                }
            }
            MessageClientTypeId::BootstrapError => {
                let (error_len, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
                cursor += delta;

                let error = String::from_utf8_lossy(
                    buffer
                        .get(cursor..cursor + error_len as usize)
                        .ok_or_else(|| {
                            ModelsError::DeserializeError(
                                "Error message content too short.".to_string(),
                            )
                        })?,
                );
                cursor += error_len as usize;

                BootstrapClientMessage::BootstrapError {
                    error: error.into_owned(),
                }
            }
        };
        Ok((res, cursor))
    }
}
