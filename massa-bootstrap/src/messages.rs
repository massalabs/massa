// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_final_state::FinalStateBootstrap;
use massa_graph::{ledger::ConsensusLedgerSubset, BootstrapableGraph};
use massa_ledger::ExecutionLedgerSubset;
use massa_models::{
    array_from_slice, constants::ADDRESS_SIZE_BYTES, Address, DeserializeCompact,
    DeserializeVarInt, ModelsError, SerializeCompact, SerializeVarInt, Version,
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
    /// Part of the ledger of consensus
    ResponseConsensusLedgerPart {
        /// Part of the consensus ledger sent
        ledger: ConsensusLedgerSubset,
    },
    /// Part of the ledger of execution
    ResponseExecutionLedgerPart {
        /// Part of the execution ledger sent
        ledger: ExecutionLedgerSubset,
    },
    /// Bootstrap error
    BootstrapError { error: String },
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u32)]
enum MessageServerTypeId {
    BootstrapTime = 0u32,
    Peers = 1u32,
    ConsensusState = 2u32,
    FinalState = 3u32,
    ResponseConsensusLedgerPart = 4u32,
    ResponseExecutionLedgerPart = 5u32,
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
            BootstrapMessageServer::ResponseConsensusLedgerPart { ledger } => {
                res.extend(u32::from(MessageServerTypeId::ResponseConsensusLedgerPart).to_varint_bytes());
                res.extend(ledger.to_bytes_compact()?);
            }
            BootstrapMessageServer::ResponseExecutionLedgerPart { ledger } => {
                res.extend(u32::from(MessageServerTypeId::ResponseExecutionLedgerPart).to_varint_bytes());
                res.extend(ledger.to_bytes_compact()?);
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
            MessageServerTypeId::ResponseConsensusLedgerPart => {
                let (ledger, delta) = ConsensusLedgerSubset::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                BootstrapMessageServer::ResponseConsensusLedgerPart { ledger }
            }
            MessageServerTypeId::ResponseExecutionLedgerPart => {
                let (ledger, delta) = ExecutionLedgerSubset::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                BootstrapMessageServer::ResponseExecutionLedgerPart { ledger }
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
    /// Ask for a part of the ledger of consensus
    AskConsensusLedgerPart {
        /// Last address sent
        address: Option<Address>,
    },
    /// Ask for a part of the ledger of execution
    AskExecutionLedgerPart {
        /// Last address sent
        address: Address,
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
    AskConsensusLedgerPart = 3u32,
    AskExecutionLedgerPart = 4u32,
    BootstrapError = 5u32,
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
            BootstrapMessageClient::AskConsensusLedgerPart { address } => {
                res.extend(u32::from(MessageClientTypeId::AskConsensusLedgerPart).to_varint_bytes());
                if let Some(address) = address {
                    res.extend(address.to_bytes());
                }
            }
            BootstrapMessageClient::AskExecutionLedgerPart { address } => {
                res.extend(u32::from(MessageClientTypeId::AskExecutionLedgerPart).to_varint_bytes());
                res.extend(address.to_bytes());
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
            MessageClientTypeId::AskConsensusLedgerPart => {
                if buffer.len() == cursor {
                    BootstrapMessageClient::AskConsensusLedgerPart { address: None }
                } else {
                    let address = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                    cursor += ADDRESS_SIZE_BYTES;

                    BootstrapMessageClient::AskConsensusLedgerPart {
                        address: Some(address),
                    }
                }
            }
            MessageClientTypeId::AskExecutionLedgerPart => {
                let address = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += ADDRESS_SIZE_BYTES;

                BootstrapMessageClient::AskExecutionLedgerPart { address }
            }
            MessageClientTypeId::BootstrapError => {
                let (error_len, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
                cursor += delta;

                let error = String::from_utf8_lossy(&buffer[cursor..cursor + error_len as usize]);
                cursor += error_len as usize;

                BootstrapMessageClient::BootstrapError {
                    error: error.into_owned(),
                }
            }
        };
        Ok((res, cursor))
    }
}