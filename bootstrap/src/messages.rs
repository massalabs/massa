use consensus::BoostrapableGraph;
use crypto::signature::{Signature, SIGNATURE_SIZE_BYTES};
use models::{
    array_from_slice, DeserializeCompact, DeserializeVarInt, ModelsError, SerializationContext,
    SerializeCompact, SerializeVarInt,
};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use serde::{Deserialize, Serialize};
use std::convert::TryInto;

pub const BOOTSTRAP_RANDOMNES_SIZE_BYTES: usize = 32;

/// Messages used during bootstrap
#[derive(Debug, Serialize, Deserialize)]
pub enum BootstrapMessage {
    /// Initiates bootstrap.
    BootstrapInitiation {
        /// Random data we expect the bootstrap node to sign with its private_key.
        random_bytes: [u8; BOOTSTRAP_RANDOMNES_SIZE_BYTES],
    },
    /// Global consensus state
    ConsensusState {
        /// Content
        graph: BoostrapableGraph,
        /// Signature of our random bytes + graph
        signature: Signature,
    },
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u32)]
enum MessageTypeId {
    BootstrapInitiation = 0u32,
    ConsensusState = 1,
}

impl SerializeCompact for BootstrapMessage {
    fn to_bytes_compact(&self, context: &SerializationContext) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();
        match self {
            BootstrapMessage::BootstrapInitiation { random_bytes } => {
                res.extend(u32::from(MessageTypeId::BootstrapInitiation).to_varint_bytes());
                res.extend(random_bytes);
            }
            BootstrapMessage::ConsensusState { graph, signature } => {
                res.extend(u32::from(MessageTypeId::ConsensusState).to_varint_bytes());
                res.extend(&signature.to_bytes());
                res.extend(&graph.to_bytes_compact(&context)?);
            }
        }
        Ok(res)
    }
}

impl DeserializeCompact for BootstrapMessage {
    fn from_bytes_compact(
        buffer: &[u8],
        context: &SerializationContext,
    ) -> Result<(Self, usize), ModelsError> {
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
                // return message
                BootstrapMessage::BootstrapInitiation { random_bytes }
            }
            MessageTypeId::ConsensusState => {
                let signature = Signature::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += SIGNATURE_SIZE_BYTES;
                let (graph, delta) =
                    BoostrapableGraph::from_bytes_compact(&buffer[cursor..], &context)?;
                cursor += delta;

                BootstrapMessage::ConsensusState { signature, graph }
            }
        };
        Ok((res, cursor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crypto::hash::Hash;
    use rand::{rngs::StdRng, RngCore, SeedableRng};

    #[test]
    fn test_message_serialize_compact() {
        //test with 2 thread
        let serialization_context = SerializationContext {
            max_block_size: 1024 * 1024,
            max_block_operations: 1024,
            parent_count: 2,
            max_peer_list_length: 128,
            max_message_size: 3 * 1024 * 1024,
        };

        let mut base_random_bytes = [0u8; 32];
        StdRng::from_entropy().fill_bytes(&mut base_random_bytes);
        let message1 = BootstrapMessage::BootstrapInitiation {
            random_bytes: base_random_bytes,
        };

        let bytes = message1.to_bytes_compact(&serialization_context).unwrap();
        let (new_message1, cursor) =
            BootstrapMessage::from_bytes_compact(&bytes, &serialization_context).unwrap();
        assert_eq!(bytes.len(), cursor);

        if let BootstrapMessage::BootstrapInitiation { random_bytes } = new_message1 {
            assert_eq!(base_random_bytes, random_bytes);
        } else {
            panic!("not the right message variant expected BootstrapInitiation");
        }

        let base_graph = BoostrapableGraph {
            /// Map of active blocks, were blocks are in their exported version.
            active_blocks: Vec::new(),
            /// Best parents hashe in each thread.
            best_parents: vec![
                Hash::hash("parent11".as_bytes()),
                Hash::hash("parent12".as_bytes()),
            ],
            /// Latest final period and block hash in each thread.
            latest_final_blocks_periods: vec![
                (Hash::hash("lfinal11".as_bytes()), 23),
                (Hash::hash("lfinal12".as_bytes()), 24),
            ],
            /// Head of the incompatibility graph.
            gi_head: vec![
                (
                    Hash::hash("gi_head11".as_bytes()),
                    vec![
                        Hash::hash("set11".as_bytes()),
                        Hash::hash("set12".as_bytes()),
                    ]
                    .into_iter()
                    .collect(),
                ),
                (
                    Hash::hash("gi_head12".as_bytes()),
                    vec![
                        Hash::hash("set21".as_bytes()),
                        Hash::hash("set22".as_bytes()),
                    ]
                    .into_iter()
                    .collect(),
                ),
                (
                    Hash::hash("gi_head13".as_bytes()),
                    vec![
                        Hash::hash("set31".as_bytes()),
                        Hash::hash("set32".as_bytes()),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ]
            .into_iter()
            .collect(),

            /// List of maximal cliques of compatible blocks.
            max_cliques: vec![vec![
                Hash::hash("max_cliques11".as_bytes()),
                Hash::hash("max_cliques12".as_bytes()),
            ]
            .into_iter()
            .collect()],
        };

        let base_signature = crypto::signature::Signature::from_bs58_check(
                    "5f4E3opXPWc3A1gvRVV7DJufvabDfaLkT1GMterpJXqRZ5B7bxPe5LoNzGDQp9LkphQuChBN1R5yEvVJqanbjx7mgLEae"
                ).unwrap();

        let message2 = BootstrapMessage::ConsensusState {
            graph: base_graph,
            signature: base_signature,
        };
        let bytes = message2.to_bytes_compact(&serialization_context).unwrap();
        let (new_message2, cursor) =
            BootstrapMessage::from_bytes_compact(&bytes, &serialization_context).unwrap();

        assert_eq!(bytes.len(), cursor);
        if let BootstrapMessage::ConsensusState { graph, signature } = new_message2 {
            assert_eq!(base_signature, signature);
            assert_eq!(Hash::hash("parent11".as_bytes()), graph.best_parents[0]);
            assert_eq!(Hash::hash("parent12".as_bytes()), graph.best_parents[1]);
        } else {
            panic!("not the right message variant expected ConsensusState");
        }
    }
}
