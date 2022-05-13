// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_final_state::FinalStateBootstrap;
use massa_graph::BootstrapableGraph;
use massa_ledger::{
    ExecutionLedgerSubset, LedgerChanges as ExecutionLedgerChanges,
    LedgerChangesDeserializer as ExecutionLedgerChangesDeserializer,
    LedgerChangesSerializer as ExecutionLedgerChangesSerializer, LedgerCursor,
    LedgerCursorDeserializer, LedgerCursorSerializer,
};
use massa_models::{
    DeserializeCompact, DeserializeVarInt, Deserializer, ModelsError, SerializeCompact,
    SerializeVarInt, Serializer, Slot, Version,
};
use massa_network_exports::BootstrapPeers;
use massa_proof_of_stake_exports::ExportProofOfStake;
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
    /// Final execution state
    FinalState {
        /// final execution state bootstrap
        final_state: FinalStateBootstrap,
    },
    /// Part of the ledger of execution
    ExecutionLedgerPart {
        /// Part of the execution ledger sent
        ledger: ExecutionLedgerSubset,
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
    FinalState = 3u32,
    ExecutionLedgerPart = 4u32,
    ExecutionLedgerFinished = 5u32,
    BootstrapError = 6u32,
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
            BootstrapMessageServer::FinalState { final_state } => {
                res.extend(u32::from(MessageServerTypeId::FinalState).to_varint_bytes());
                res.extend(&final_state.to_bytes_compact()?);
            }
            BootstrapMessageServer::ExecutionLedgerPart {
                ledger,
                slot,
                ledger_changes,
            } => {
                res.extend(u32::from(MessageServerTypeId::ExecutionLedgerPart).to_varint_bytes());
                res.extend(ledger.to_bytes_compact()?);
                res.extend(slot.to_bytes_compact()?);
                let serializer = ExecutionLedgerChangesSerializer::new();
                res.extend(serializer.serialize(ledger_changes)?);
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
            MessageServerTypeId::FinalState => {
                let (final_state, delta) =
                    FinalStateBootstrap::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                BootstrapMessageServer::FinalState { final_state }
            }
            MessageServerTypeId::ExecutionLedgerPart => {
                let (ledger, delta) = ExecutionLedgerSubset::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                let (slot, delta) = Slot::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;
                let deserializer = ExecutionLedgerChangesDeserializer::new();
                let (ledger_changes, delta) = deserializer.deserialize(&buffer[cursor..])?;
                cursor += delta;

                BootstrapMessageServer::ExecutionLedgerPart {
                    ledger,
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
#[derive(Debug, Clone)]
pub enum BootstrapMessageClient {
    /// Ask for bootstrap peers
    AskBootstrapPeers,
    /// Ask for consensus state
    AskConsensusState,
    /// Ask for final state
    AskFinalState,
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
    AskFinalState = 2u32,
    AskExecutionLedgerPart = 3u32,
    BootstrapError = 4u32,
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
            BootstrapMessageClient::AskFinalState => {
                res.extend(u32::from(MessageClientTypeId::AskFinalState).to_varint_bytes());
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
            MessageClientTypeId::AskFinalState => BootstrapMessageClient::AskFinalState,
            MessageClientTypeId::AskExecutionLedgerPart => {
                if buffer.len() == cursor {
                    BootstrapMessageClient::AskExecutionLedgerPart {
                        cursor: None,
                        slot: None,
                    }
                } else {
                    let cursor_deserializer = LedgerCursorDeserializer::new();
                    let (bootstrap_cursor, delta) =
                        cursor_deserializer.deserialize(&buffer[cursor..])?;
                    cursor += delta;

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
