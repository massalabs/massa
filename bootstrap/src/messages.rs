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
    fn test_unserialize_compact() {
        //    msg len 605
        let msg_buffer: Vec<u8> = vec![
            1, 75, 81, 52, 212, 238, 82, 236, 87, 11, 87, 243, 181, 234, 79, 231, 252, 99, 162,
            151, 192, 229, 71, 112, 125, 179, 221, 39, 48, 235, 152, 0, 56, 15, 79, 92, 161, 196,
            72, 235, 226, 45, 128, 58, 16, 43, 174, 185, 66, 223, 221, 227, 245, 86, 176, 175, 201,
            7, 144, 74, 22, 112, 67, 157, 210, 2, 45, 18, 193, 241, 163, 49, 105, 140, 117, 75,
            235, 220, 237, 36, 190, 23, 24, 131, 7, 113, 129, 166, 37, 168, 240, 63, 237, 217, 141,
            254, 21, 53, 1, 3, 47, 111, 204, 179, 71, 255, 71, 61, 119, 184, 53, 120, 19, 38, 72,
            11, 108, 68, 147, 252, 172, 226, 153, 19, 174, 106, 225, 115, 255, 68, 54, 110, 0, 1,
            47, 149, 29, 58, 223, 41, 171, 37, 77, 115, 66, 134, 117, 94, 33, 49, 195, 151, 182,
            252, 24, 148, 230, 255, 229, 178, 54, 234, 94, 9, 158, 207, 47, 149, 29, 58, 223, 41,
            171, 37, 77, 115, 66, 134, 117, 94, 33, 49, 195, 151, 182, 252, 24, 148, 230, 255, 229,
            178, 54, 234, 94, 9, 158, 207, 209, 46, 52, 137, 111, 156, 57, 35, 72, 7, 86, 157, 106,
            157, 139, 155, 243, 93, 103, 6, 134, 242, 212, 177, 115, 98, 126, 181, 176, 106, 235,
            60, 19, 75, 107, 10, 193, 186, 128, 169, 152, 81, 199, 55, 42, 241, 151, 107, 35, 55,
            52, 22, 146, 72, 35, 235, 202, 19, 234, 94, 37, 186, 245, 34, 0, 0, 0, 2, 0, 0, 0, 158,
            0, 185, 203, 173, 227, 191, 74, 127, 117, 135, 168, 74, 14, 189, 183, 135, 14, 223,
            244, 139, 188, 240, 174, 90, 84, 220, 52, 8, 91, 88, 213, 1, 3, 47, 111, 204, 179, 71,
            255, 71, 61, 119, 184, 53, 120, 19, 38, 72, 11, 108, 68, 147, 252, 172, 226, 153, 19,
            174, 106, 225, 115, 255, 68, 54, 110, 0, 0, 47, 149, 29, 58, 223, 41, 171, 37, 77, 115,
            66, 134, 117, 94, 33, 49, 195, 151, 182, 252, 24, 148, 230, 255, 229, 178, 54, 234, 94,
            9, 158, 207, 47, 149, 29, 58, 223, 41, 171, 37, 77, 115, 66, 134, 117, 94, 33, 49, 195,
            151, 182, 252, 24, 148, 230, 255, 229, 178, 54, 234, 94, 9, 158, 207, 96, 1, 176, 31,
            255, 94, 153, 25, 115, 121, 45, 189, 109, 178, 154, 241, 105, 130, 246, 52, 178, 235,
            113, 105, 103, 250, 194, 241, 34, 168, 115, 22, 67, 237, 89, 52, 235, 31, 5, 13, 41,
            175, 80, 206, 27, 179, 80, 197, 168, 98, 146, 209, 23, 111, 21, 6, 98, 136, 121, 168,
            75, 13, 189, 233, 0, 0, 0, 2, 0, 0, 0, 158, 0, 185, 203, 173, 227, 191, 74, 127, 117,
            135, 168, 74, 14, 189, 183, 135, 14, 223, 244, 139, 188, 240, 174, 90, 84, 220, 52, 8,
            91, 88, 213, 45, 18, 193, 241, 163, 49, 105, 140, 117, 75, 235, 220, 237, 36, 190, 23,
            24, 131, 7, 113, 129, 166, 37, 168, 240, 63, 237, 217, 141, 254, 21, 53, 158, 0, 185,
            203, 173, 227, 191, 74, 127, 117, 135, 168, 74, 14, 189, 183, 135, 14, 223, 244, 139,
            188, 240, 174, 90, 84, 220, 52, 8, 91, 88, 213, 0, 45, 18, 193, 241, 163, 49, 105, 140,
            117, 75, 235, 220, 237, 36, 190, 23, 24, 131, 7, 113, 129, 166, 37, 168, 240, 63, 237,
            217, 141, 254, 21, 53, 0, 0, 1, 0,
        ];
        let serialization_context = SerializationContext {
            max_block_size: 1024 * 1024,
            max_block_operations: 1024,
            parent_count: 2,
            max_peer_list_length: 128,
            max_message_size: 3 * 1024 * 1024,
        };
        let (new_message1, cursor) =
            BootstrapMessage::from_bytes_compact(&msg_buffer, &serialization_context).unwrap();

        println!(" new_message1{:?}", new_message1);
    }
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
