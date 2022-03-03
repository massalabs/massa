// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::constants::{BLOCK_ID_SIZE_BYTES, ENDORSEMENT_ID_SIZE_BYTES};
use crate::prehash::PreHashed;
use crate::signed::{Id, Signable};
use crate::{
    serialization::{
        array_from_slice, DeserializeCompact, DeserializeVarInt, SerializeCompact, SerializeVarInt,
    },
    with_serialization_context, BlockId, ModelsError, Slot,
};
use massa_hash::hash::Hash;
use massa_signature::{PublicKey, PUBLIC_KEY_SIZE_BYTES};
use serde::{Deserialize, Serialize};
use std::{fmt::Display, str::FromStr};

const ENDORSEMENT_ID_STRING_PREFIX: &str = "END";
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct EndorsementId(Hash);

impl PreHashed for EndorsementId {}
impl Id for EndorsementId {
    fn new(hash: Hash) -> Self {
        EndorsementId(hash)
    }
}

impl std::fmt::Display for EndorsementId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if cfg!(feature = "hash-prefix") {
            write!(
                f,
                "{}-{}",
                ENDORSEMENT_ID_STRING_PREFIX,
                self.0.to_bs58_check()
            )
        } else {
            write!(f, "{}", self.0.to_bs58_check())
        }
    }
}

impl FromStr for EndorsementId {
    type Err = ModelsError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if cfg!(feature = "hash-prefix") {
            let v: Vec<_> = s.split('-').collect();
            if v.len() != 2 {
                // assume there is no prefix
                Ok(EndorsementId(Hash::from_str(s)?))
            } else if v[0] != ENDORSEMENT_ID_STRING_PREFIX {
                Err(ModelsError::WrongPrefix(
                    ENDORSEMENT_ID_STRING_PREFIX.to_string(),
                    v[0].to_string(),
                ))
            } else {
                Ok(EndorsementId(Hash::from_str(v[1])?))
            }
        } else {
            Ok(EndorsementId(Hash::from_str(s)?))
        }
    }
}

impl EndorsementId {
    pub fn to_bytes(&self) -> [u8; ENDORSEMENT_ID_SIZE_BYTES] {
        self.0.to_bytes()
    }

    pub fn into_bytes(self) -> [u8; ENDORSEMENT_ID_SIZE_BYTES] {
        self.0.into_bytes()
    }

    pub fn from_bytes(
        data: &[u8; ENDORSEMENT_ID_SIZE_BYTES],
    ) -> Result<EndorsementId, ModelsError> {
        Ok(EndorsementId(
            Hash::from_bytes(data).map_err(|_| ModelsError::HashError)?,
        ))
    }
    pub fn from_bs58_check(data: &str) -> Result<EndorsementId, ModelsError> {
        Ok(EndorsementId(
            Hash::from_bs58_check(data).map_err(|_| ModelsError::HashError)?,
        ))
    }
}

// #[derive(Debug, Clone, Serialize, Deserialize)]
// pub struct Endorsement {
//     pub content: Endorsement,
//     pub signature: Signature,
// }

impl Display for Endorsement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Endorsed block: {} at slot {}",
            self.endorsed_block, self.slot
        )?;
        writeln!(f, "Index: {}", self.index)?;
        writeln!(
            f,
            "Endorsement creator public key: {}",
            self.sender_public_key
        )?;
        Ok(())
    }
}

impl Endorsement {
    pub fn compute_hash(&self) -> Result<Hash, ModelsError> {
        Ok(Hash::compute_from(&self.to_bytes_compact()?))
    }
}

// impl Endorsement {
//     /// Verify the signature and integrity of the endorsement and computes ID
//     pub fn verify_signature(&self) -> Result<(), ModelsError> {
//         let content_hash = Hash::compute_from(&self.content.to_bytes_compact()?);
//         verify_signature(
//             &content_hash,
//             &self.signature,
//             &self.content.sender_public_key,
//         )?;
//         Ok(())
//     }

//     pub fn new_signed(
//         private_key: &PrivateKey,
//         content: Endorsement,
//     ) -> Result<(EndorsementId, Self), ModelsError> {
//         let content_hash = content.compute_hash()?;
//         let signature = sign(&content_hash, private_key)?;
//         let endorsement = Endorsement { content, signature };
//         let e_id = endorsement.compute_id()?;
//         Ok((e_id, endorsement))
//     }

//     pub fn compute_endorsement_id(&self) -> Result<EndorsementId, ModelsError> {
//         Ok(EndorsementId(Hash::compute_from(&self.to_bytes_compact()?)))
//     }
// }

// /// Checks performed:
// /// - Validity of the content.
// impl SerializeCompact for Endorsement {
//     fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
//         let mut res: Vec<u8> = Vec::new();

//         // content
//         res.extend(self.content.to_bytes_compact()?);

//         // signature
//         res.extend(&self.signature.to_bytes());

//         Ok(res)
//     }
// }

// /// Checks performed:
// /// - Validity of the content.
// /// - Validity of the signature.
// impl DeserializeCompact for Endorsement {
//     fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
//         let mut cursor = 0;

//         // content
//         let (content, delta) = Endorsement::from_bytes_compact(&buffer[cursor..])?;
//         cursor += delta;

//         // signature
//         let signature = Signature::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
//         cursor += SIGNATURE_SIZE_BYTES;

//         let res = Endorsement { content, signature };

//         Ok((res, cursor))
//     }
// }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endorsement {
    /// Public key of the endorser.
    pub sender_public_key: PublicKey,
    /// slot of endorsed block
    pub slot: Slot,
    /// endorsement index inside the block
    pub index: u32,
    /// hash of endorsed block
    pub endorsed_block: BlockId,
}

impl Signable<EndorsementId> for Endorsement {
    fn get_signature_message(&self) -> Result<Hash, ModelsError> {
        self.compute_hash()
    }
}

/// Checks performed:
/// - Validity of the slot.
impl SerializeCompact for Endorsement {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // Sender public key
        res.extend(&self.sender_public_key.to_bytes());

        // Slot
        res.extend(self.slot.to_bytes_compact()?);

        // endorsement index inside the block
        res.extend(self.index.to_varint_bytes());

        // id of endorsed block
        res.extend(&self.endorsed_block.to_bytes());

        Ok(res)
    }
}

/// Checks performed:
/// - Validity of the sender public key.
/// - Validity of the slot.
/// - Validity of the endorsement index.
/// - Validity of the endorsed block id.
impl DeserializeCompact for Endorsement {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let max_block_endorsements =
            with_serialization_context(|context| context.endorsement_count);
        let mut cursor = 0usize;

        // sender public key
        let sender_public_key = PublicKey::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        cursor += PUBLIC_KEY_SIZE_BYTES;

        // slot
        let (slot, delta) = Slot::from_bytes_compact(&buffer[cursor..])?;
        if slot.period == 0 {
            return Err(ModelsError::DeserializeError(
                "the target period of an endorsement cannot be 0".into(),
            ));
        }
        cursor += delta;

        // endorsement index inside the block
        let (index, delta) = u32::from_varint_bytes_bounded(
            &buffer[cursor..],
            max_block_endorsements.saturating_sub(1),
        )?;
        cursor += delta;

        // id of endorsed block
        let endorsed_block = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        cursor += BLOCK_ID_SIZE_BYTES;

        Ok((
            Endorsement {
                sender_public_key,
                slot,
                index,
                endorsed_block,
            },
            cursor,
        ))
    }
}

#[cfg(test)]
mod tests {
    use crate::signed::Signed;

    use super::*;
    use massa_signature::{derive_public_key, generate_random_private_key};
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_endorsement_serialization() {
        let ctx = crate::SerializationContext {
            max_block_size: 1024 * 1024,
            max_operations_per_block: 1024,
            thread_count: 3,
            max_advertise_length: 128,
            max_message_size: 3 * 1024 * 1024,
            max_bootstrap_blocks: 100,
            max_bootstrap_cliques: 100,
            max_bootstrap_deps: 100,
            max_bootstrap_children: 100,
            max_bootstrap_pos_cycles: 1000,
            max_bootstrap_pos_entries: 1000,
            max_ask_blocks_per_message: 10,
            max_operations_per_message: 1024,
            max_endorsements_per_message: 1024,
            max_bootstrap_message_size: 100000000,
            endorsement_count: 8,
        };
        crate::init_serialization_context(ctx);

        let sender_priv = generate_random_private_key();
        let sender_public_key = derive_public_key(&sender_priv);

        let content = Endorsement {
            sender_public_key,
            slot: Slot::new(10, 1),
            index: 0,
            endorsed_block: BlockId(Hash::compute_from("blk".as_bytes())),
        };
        let endorsement = Signed::new_signed(content.clone(), &sender_priv).unwrap().1;

        let ser_content = content.to_bytes_compact().unwrap();
        let (res_content, _) = Endorsement::from_bytes_compact(&ser_content).unwrap();
        assert_eq!(format!("{:?}", res_content), format!("{:?}", content));
        let ser_endorsement = endorsement.to_bytes_compact().unwrap();
        let (res_endorsement, _) =
            Signed::<Endorsement, EndorsementId>::from_bytes_compact(&ser_endorsement).unwrap();
        assert_eq!(
            format!("{:?}", res_endorsement),
            format!("{:?}", endorsement)
        );
    }
}
