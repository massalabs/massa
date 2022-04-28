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

/// Messages used during bootstrap
#[derive(Debug, Clone)]
pub enum BootstrapMessage {
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
    /// Ask for a part of the ledger of consensus
    AskConsensusLedgerPart {
        /// Last address sent
        address: Address,
    },
    /// Part of the ledger of consensus
    ResponseConsensusLedgerPart {
        /// Part of the consensus ledger sent
        ledger: ConsensusLedgerSubset,
    },
    /// Ask for a part of the ledger of execution
    AskExecutionLedgerPart {
        /// Last address sent
        address: Address,
    },
    /// Part of the ledger of execution
    ResponseExecutionLedgerPart {
        /// Part of the execution ledger sent
        ledger: ExecutionLedgerSubset,
    },
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u32)]
enum MessageTypeId {
    BootstrapTime = 0u32,
    Peers = 1u32,
    ConsensusState = 2u32,
    FinalState = 3u32,
    AskConsensusLedgerPart = 4u32,
    ResponseConsensusLedgerPart = 5u32,
    AskExecutionLedgerPart = 6u32,
    ResponseExecutionLedgerPart = 7u32,
}

impl SerializeCompact for BootstrapMessage {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();
        match self {
            BootstrapMessage::BootstrapTime {
                server_time,
                version,
            } => {
                res.extend(u32::from(MessageTypeId::BootstrapTime).to_varint_bytes());
                res.extend(server_time.to_bytes_compact()?);
                res.extend(&version.to_bytes_compact()?)
            }
            BootstrapMessage::BootstrapPeers { peers } => {
                res.extend(u32::from(MessageTypeId::Peers).to_varint_bytes());
                res.extend(&peers.to_bytes_compact()?);
            }
            BootstrapMessage::ConsensusState { pos, graph } => {
                res.extend(u32::from(MessageTypeId::ConsensusState).to_varint_bytes());
                res.extend(&pos.to_bytes_compact()?);
                res.extend(&graph.to_bytes_compact()?);
            }
            BootstrapMessage::FinalState { final_state } => {
                res.extend(u32::from(MessageTypeId::FinalState).to_varint_bytes());
                res.extend(&final_state.to_bytes_compact()?);
            }
            BootstrapMessage::AskConsensusLedgerPart { address } => {
                res.extend(address.to_bytes());
            }
            BootstrapMessage::AskExecutionLedgerPart { address } => {
                res.extend(address.to_bytes());
            }
            BootstrapMessage::ResponseConsensusLedgerPart { ledger } => {
                res.extend(ledger.to_bytes_compact()?);
            }
            BootstrapMessage::ResponseExecutionLedgerPart { ledger } => {
                res.extend(ledger.to_bytes_compact()?);
            }
        }
        Ok(res)
    }
}

impl DeserializeCompact for BootstrapMessage {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0usize;

        let (type_id_raw, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        let type_id: MessageTypeId = type_id_raw
            .try_into()
            .map_err(|_| ModelsError::DeserializeError("invalid message type ID".into()))?;

        let res = match type_id {
            MessageTypeId::BootstrapTime => {
                let (server_time, delta) = MassaTime::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                let (version, delta) = Version::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;
                BootstrapMessage::BootstrapTime {
                    server_time,
                    version,
                }
            }
            MessageTypeId::Peers => {
                let (peers, delta) = BootstrapPeers::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                BootstrapMessage::BootstrapPeers { peers }
            }
            MessageTypeId::ConsensusState => {
                let (pos, delta) = ExportProofOfStake::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;
                let (graph, delta) = BootstrapableGraph::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                BootstrapMessage::ConsensusState { pos, graph }
            }
            MessageTypeId::FinalState => {
                let (final_state, delta) =
                    FinalStateBootstrap::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                BootstrapMessage::FinalState { final_state }
            }
            MessageTypeId::AskConsensusLedgerPart => {
                let address = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += ADDRESS_SIZE_BYTES;

                BootstrapMessage::AskConsensusLedgerPart { address }
            }
            MessageTypeId::ResponseConsensusLedgerPart => {
                let (ledger, delta) = ConsensusLedgerSubset::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                BootstrapMessage::ResponseConsensusLedgerPart { ledger }
            }
            MessageTypeId::AskExecutionLedgerPart => {
                let address = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += ADDRESS_SIZE_BYTES;

                BootstrapMessage::AskExecutionLedgerPart { address }
            }
            MessageTypeId::ResponseExecutionLedgerPart => {
                let (ledger, delta) = ExecutionLedgerSubset::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                BootstrapMessage::ResponseExecutionLedgerPart { ledger }
            }
        };
        Ok((res, cursor))
    }
}
