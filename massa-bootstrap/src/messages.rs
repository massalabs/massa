// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_graph::BootstrapableGraph;
use massa_ledger::FinalLedgerBootstrapState;
use massa_models::{
    DeserializeCompact, DeserializeVarInt, ModelsError, SerializeCompact, SerializeVarInt, Version,
};
use massa_network::BootstrapPeers;
use massa_proof_of_stake_exports::ExportProofOfStake;
use massa_time::MassaTime;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use serde::{Deserialize, Serialize};
use std::convert::TryInto;

/// Messages used during bootstrap
#[derive(Debug, Serialize, Deserialize)]
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
    /// Execution state
    FinalLedgerState {
        /// ledger state
        ledger_state: FinalLedgerBootstrapState,
    },
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u32)]
enum MessageTypeId {
    BootstrapTime = 0u32,
    Peers = 1u32,
    ConsensusState = 2u32,
    FinalLedgerState = 3u32,
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
            BootstrapMessage::FinalLedgerState { ledger_state } => {
                res.extend(u32::from(MessageTypeId::FinalLedgerState).to_varint_bytes());
                res.extend(&ledger_state.to_bytes_compact()?);
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
            MessageTypeId::FinalLedgerState => {
                let (ledger_state, delta) =
                    FinalLedgerBootstrapState::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                BootstrapMessage::FinalLedgerState { ledger_state }
            }
        };
        Ok((res, cursor))
    }
}
