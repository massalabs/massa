// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_graph::BootstrapableGraph;
use massa_ledger::{
    LedgerChanges as ExecutionLedgerChanges,
    LedgerChangesDeserializer as ExecutionLedgerChangesDeserializer,
    LedgerChangesSerializer as ExecutionLedgerChangesSerializer, LedgerCursor,
    LedgerCursorDeserializer, LedgerCursorSerializer,
};
use massa_models::{
    DeserializeCompact, DeserializeVarInt, ModelsError, SerializeCompact, SerializeVarInt, Slot,
    Version,
};
use massa_network_exports::BootstrapPeers;
use massa_proof_of_stake_exports::ExportProofOfStake;
use massa_serialization::{Deserializer, Serializer};
use massa_time::MassaTime;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use std::convert::TryInto;

/// Messages used during bootstrap by server
#[derive(Debug, Clone)]
pub enum BootstrapMessageServer {
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
    ExecutionLedgerPart {
        /// Part of the execution ledger sent in a serialized way
        data: Vec<u8>,
        /// Slot the ledger changes are attached to
        slot: Slot,
        /// Ledger change for addresses inferior to `address` of the client message.
        ledger_changes: ExecutionLedgerChanges,
    },
    ExecutionLedgerFinished,
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
    ExecutionLedgerPart = 3u32,
    ExecutionLedgerFinished = 4u32,
    BootstrapError = 5u32,
}

impl SerializeCompact for BootstrapMessageServer {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();
        match self {
            BootstrapMessageServer::BootstrapTime {
                server_time,
                version,
            } => {
                res.extend(u32::from(MessageServerTypeId::BootstrapTime).to_varint_bytes());
                res.extend(server_time.to_bytes_compact()?);
                res.extend(&version.to_bytes_compact()?)
            }
            BootstrapMessageServer::BootstrapPeers { peers } => {
                res.extend(u32::from(MessageServerTypeId::Peers).to_varint_bytes());
                res.extend(&peers.to_bytes_compact()?);
            }
            BootstrapMessageServer::ConsensusState { pos, graph } => {
                res.extend(u32::from(MessageServerTypeId::ConsensusState).to_varint_bytes());
                res.extend(&pos.to_bytes_compact()?);
                res.extend(&graph.to_bytes_compact()?);
            }
            BootstrapMessageServer::ExecutionLedgerPart {
                data,
                slot,
                ledger_changes,
            } => {
                let ledger_execution_serializer = ExecutionLedgerChangesSerializer::new();
                res.extend(u32::from(MessageServerTypeId::ExecutionLedgerPart).to_varint_bytes());
                res.extend((data.len() as u64).to_varint_bytes());
                res.extend(data);
                res.extend(slot.to_bytes_compact()?);
                res.extend(ledger_execution_serializer.serialize(ledger_changes)?);
            }
            BootstrapMessageServer::ExecutionLedgerFinished => {
                res.extend(
                    u32::from(MessageServerTypeId::ExecutionLedgerFinished).to_varint_bytes(),
                );
            }
            BootstrapMessageServer::BootstrapError { error } => {
                res.extend(u32::from(MessageServerTypeId::BootstrapError).to_varint_bytes());
                res.extend(u32::to_varint_bytes(error.len() as u32));
                res.extend(error.as_bytes())
            }
        }
        Ok(res)
    }
}

impl DeserializeCompact for BootstrapMessageServer {
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
                BootstrapMessageServer::BootstrapTime {
                    server_time,
                    version,
                }
            }
            MessageServerTypeId::Peers => {
                let (peers, delta) = BootstrapPeers::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                BootstrapMessageServer::BootstrapPeers { peers }
            }
            MessageServerTypeId::ConsensusState => {
                let (pos, delta) = ExportProofOfStake::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;
                let (graph, delta) = BootstrapableGraph::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                BootstrapMessageServer::ConsensusState { pos, graph }
            }
            MessageServerTypeId::ExecutionLedgerPart => {
                let cursor_deserializer = LedgerCursorDeserializer::new();
                let ledger_execution_deserializer = ExecutionLedgerChangesDeserializer::new();

                let (data_len, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
                cursor += delta;

                let data = &buffer[cursor..cursor + data_len as usize];
                cursor += data_len as usize;

                let (rest, ledger_cursor) = cursor_deserializer.deserialize(&buffer[cursor..])?;
                cursor += rest.len();

                let (slot, delta) = Slot::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;
                let (rest, ledger_changes) =
                    ledger_execution_deserializer.deserialize(&buffer[cursor..])?;
                cursor += rest.len();

                BootstrapMessageServer::ExecutionLedgerPart {
                    data: data.to_vec(),
                    slot,
                    ledger_changes,
                }
            }
            MessageServerTypeId::ExecutionLedgerFinished => {
                BootstrapMessageServer::ExecutionLedgerFinished
            }
            MessageServerTypeId::BootstrapError => {
                let (error_len, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
                cursor += delta;

                let error = String::from_utf8_lossy(&buffer[cursor..cursor + error_len as usize]);
                cursor += error_len as usize;

                BootstrapMessageServer::BootstrapError {
                    error: error.into_owned(),
                }
            }
        };
        Ok((res, cursor))
    }
}

/// Messages used during bootstrap by client
#[derive(Debug)]
pub enum BootstrapMessageClient {
    /// Ask for bootstrap peers
    AskBootstrapPeers,
    /// Ask for consensus state
    AskConsensusState,
    /// Ask for a part of the ledger of execution
    AskExecutionLedgerPart {
        /// Last position of the cursor received from the server
        cursor: Option<LedgerCursor>,
        /// Slot we are attached to for ledger changes
        slot: Option<Slot>,
    },
    /// Bootstrap error
    BootstrapError { error: String },
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u32)]
enum MessageClientTypeId {
    AskBootstrapPeers = 0u32,
    AskConsensusState = 1u32,
    AskExecutionLedgerPart = 2u32,
    BootstrapError = 3u32,
}

impl SerializeCompact for BootstrapMessageClient {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();
        match self {
            BootstrapMessageClient::AskBootstrapPeers => {
                res.extend(u32::from(MessageClientTypeId::AskBootstrapPeers).to_varint_bytes());
            }
            BootstrapMessageClient::AskConsensusState => {
                res.extend(u32::from(MessageClientTypeId::AskConsensusState).to_varint_bytes());
            }
            BootstrapMessageClient::AskExecutionLedgerPart { cursor, slot } => {
                res.extend(
                    u32::from(MessageClientTypeId::AskExecutionLedgerPart).to_varint_bytes(),
                );
                // If we have a cursor we must have also a slot
                if let Some(cursor) = cursor && let Some(slot) = slot  {
                    let cursor_serializer = LedgerCursorSerializer::new();
                    res.extend(cursor_serializer.serialize(cursor)?);
                    res.extend(slot.to_bytes_compact()?);
                }
            }
            BootstrapMessageClient::BootstrapError { error } => {
                res.extend(u32::from(MessageClientTypeId::BootstrapError).to_varint_bytes());
                res.extend(u32::to_varint_bytes(error.len() as u32));
                res.extend(error.as_bytes())
            }
        }
        Ok(res)
    }
}

impl DeserializeCompact for BootstrapMessageClient {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0usize;

        let (type_id_raw, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        let type_id: MessageClientTypeId = type_id_raw
            .try_into()
            .map_err(|_| ModelsError::DeserializeError("invalid message type ID".into()))?;

        let res = match type_id {
            MessageClientTypeId::AskBootstrapPeers => BootstrapMessageClient::AskBootstrapPeers,
            MessageClientTypeId::AskConsensusState => BootstrapMessageClient::AskConsensusState,
            MessageClientTypeId::AskExecutionLedgerPart => {
                if buffer.len() == cursor {
                    BootstrapMessageClient::AskExecutionLedgerPart {
                        cursor: None,
                        slot: None,
                    }
                } else {
                    let cursor_deserializer = LedgerCursorDeserializer::new();
                    let (rest, bootstrap_cursor) =
                        cursor_deserializer.deserialize(&buffer[cursor..])?;
                    cursor += rest.len();

                    let (slot, delta) = Slot::from_bytes_compact(&buffer[cursor..])?;
                    cursor += delta;

                    BootstrapMessageClient::AskExecutionLedgerPart {
                        cursor: Some(bootstrap_cursor),
                        slot: Some(slot),
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

                BootstrapMessageClient::BootstrapError {
                    error: error.into_owned(),
                }
            }
        };
        Ok((res, cursor))
    }
}
