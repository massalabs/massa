// Copyright (c) 2021 MASSA LABS <info@massa.net>

use communication::network::BootstrapPeers;
use consensus::{BootstrapableGraph, ExportProofOfStake};
use crypto::signature::{Signature, SIGNATURE_SIZE_BYTES};
use models::{
    array_from_slice, DeserializeCompact, DeserializeVarInt, ModelsError, SerializeCompact,
    SerializeVarInt, Version,
};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use serde::{Deserialize, Serialize};
use std::convert::TryInto;
use time::UTime;

pub const BOOTSTRAP_RANDOMNES_SIZE_BYTES: usize = 32;

/// Messages used during bootstrap
#[derive(Debug, Serialize, Deserialize)]
pub enum BootstrapMessage {
    /// Initiates bootstrap.
    BootstrapInitiation {
        /// Random data we expect the bootstrap node to sign with its private_key.
        random_bytes: [u8; BOOTSTRAP_RANDOMNES_SIZE_BYTES],
        version: Version,
    },
    /// Sync clocks,
    BootstrapTime {
        /// The current time on the bootstrap server.
        server_time: UTime,
        version: Version,
        /// Signature of [BootstrapInitiation.random_bytes + server_time].
        signature: Signature,
    },
    /// Sync clocks,
    BootstrapPeers {
        /// Server peers
        peers: BootstrapPeers,
        /// Signature of [BootstrapTime.signature + peers]
        signature: Signature,
    },
    /// Global consensus state
    ConsensusState {
        /// PoS
        pos: ExportProofOfStake,
        /// block graph
        graph: BootstrapableGraph,
        /// Signature of [BootstrapPeers.signature + peers]
        signature: Signature,
    },
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u32)]
enum MessageTypeId {
    BootstrapInitiation = 0u32,
    BootstrapTime = 1,
    Peers = 2,
    ConsensusState = 3,
}

impl SerializeCompact for BootstrapMessage {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();
        match self {
            BootstrapMessage::BootstrapInitiation {
                random_bytes,
                version,
            } => {
                res.extend(u32::from(MessageTypeId::BootstrapInitiation).to_varint_bytes());
                res.extend(random_bytes);
                res.extend(&version.to_bytes_compact()?)
            }
            BootstrapMessage::BootstrapTime {
                server_time,
                version,
                signature,
            } => {
                res.extend(u32::from(MessageTypeId::BootstrapTime).to_varint_bytes());
                res.extend(&signature.to_bytes());
                res.extend(server_time.to_bytes_compact()?);
                res.extend(&version.to_bytes_compact()?)
            }
            BootstrapMessage::BootstrapPeers { peers, signature } => {
                res.extend(u32::from(MessageTypeId::Peers).to_varint_bytes());
                res.extend(&signature.to_bytes());
                res.extend(&peers.to_bytes_compact()?);
            }
            BootstrapMessage::ConsensusState {
                pos,
                graph,
                signature,
            } => {
                res.extend(u32::from(MessageTypeId::ConsensusState).to_varint_bytes());
                res.extend(&signature.to_bytes());
                res.extend(&pos.to_bytes_compact()?);
                res.extend(&graph.to_bytes_compact()?);
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
            MessageTypeId::BootstrapInitiation => {
                // random bytes
                let random_bytes: [u8; BOOTSTRAP_RANDOMNES_SIZE_BYTES] =
                    array_from_slice(&buffer[cursor..])?;
                cursor += BOOTSTRAP_RANDOMNES_SIZE_BYTES;

                //version
                let (version, delta) = Version::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;
                // return message
                BootstrapMessage::BootstrapInitiation {
                    random_bytes,
                    version,
                }
            }
            MessageTypeId::BootstrapTime => {
                let signature = Signature::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += SIGNATURE_SIZE_BYTES;
                let (server_time, delta) = UTime::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                let (version, delta) = Version::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;
                BootstrapMessage::BootstrapTime {
                    server_time,
                    signature,
                    version,
                }
            }
            MessageTypeId::Peers => {
                let signature = Signature::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += SIGNATURE_SIZE_BYTES;
                let (peers, delta) = BootstrapPeers::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                BootstrapMessage::BootstrapPeers { signature, peers }
            }
            MessageTypeId::ConsensusState => {
                let signature = Signature::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += SIGNATURE_SIZE_BYTES;
                let (pos, delta) = ExportProofOfStake::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;
                let (graph, delta) = BootstrapableGraph::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                BootstrapMessage::ConsensusState {
                    pos,
                    signature,
                    graph,
                }
            }
        };
        Ok((res, cursor))
    }
}
