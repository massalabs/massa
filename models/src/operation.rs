use crate::{
    serialization::{
        array_from_slice, DeserializeCompact, DeserializeVarInt, SerializeCompact, SerializeVarInt,
    },
    ModelsError, SerializationContext,
};
use crypto::{
    hash::{Hash, HASH_SIZE_BYTES},
    signature::{PublicKey, Signature, PUBLIC_KEY_SIZE_BYTES, SIGNATURE_SIZE_BYTES},
};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use serde::{Deserialize, Serialize};
use std::convert::TryInto;

#[derive(Eq, PartialEq, Ord, PartialOrd, Copy, Clone, Hash, Debug, Serialize, Deserialize)]
pub struct Address(Hash); // Public key hash

impl Address {
    pub fn new(key: PublicKey) -> Address {
        Address(Hash::hash(&key.to_bytes()[..]))
    }

    /// Assumes that thread count is a power of two
    pub fn get_thread(&self, thread_count: u8) -> u8 {
        let hash_bytes = self.0.to_bytes();
        hash_bytes[0] >> (8 - thread_count.trailing_zeros())
    }
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u32)]
enum OperationTypeId {
    Transaction = 0,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Operation {
    pub content: OperationContent,
    pub signature: Signature,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct OperationContent {
    pub fee: u64,
    pub expiration_period: u64,
    pub creator_public_key: PublicKey,
    pub op: OperationType,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum OperationType {
    Transaction {
        recipient_address: Address, // Hash of the recipient's public key
        amount: u64,
    },
}

impl SerializeCompact for OperationContent {
    fn to_bytes_compact(&self, _context: &SerializationContext) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // fee
        res.extend(self.fee.to_varint_bytes());

        // expire period
        res.extend(self.expiration_period.to_varint_bytes());

        // sender public key
        res.extend(&self.creator_public_key.to_bytes());

        match self.op {
            OperationType::Transaction {
                recipient_address,
                amount,
            } => {
                // type id
                res.extend(u32::from(OperationTypeId::Transaction).to_varint_bytes());
                // recipient address
                res.extend(&recipient_address.0.to_bytes());

                // amount
                res.extend(amount.to_varint_bytes());
            }
        }
        Ok(res)
    }
}

impl DeserializeCompact for OperationContent {
    fn from_bytes_compact(
        buffer: &[u8],
        _context: &SerializationContext,
    ) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0usize;

        // fee
        let (fee, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        // expire period
        let (expiration_period, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        // sender public key
        let creator_public_key = PublicKey::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        cursor += PUBLIC_KEY_SIZE_BYTES;

        let (type_id_raw, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        let type_id: OperationTypeId = type_id_raw
            .try_into()
            .map_err(|_| ModelsError::DeserializeError("invalid message type ID".into()))?;

        let op = match type_id {
            OperationTypeId::Transaction => {
                // recipient address
                let recipient_address = Hash::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += HASH_SIZE_BYTES;

                // amount
                let (amount, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
                cursor += delta;

                OperationType::Transaction {
                    recipient_address: Address(recipient_address),
                    amount,
                }
            }
        };

        Ok((
            OperationContent {
                fee,
                expiration_period,
                creator_public_key,
                op,
            },
            cursor,
        ))
    }
}
impl OperationContent {
    pub fn compute_hash(&self, context: &SerializationContext) -> Result<Hash, ModelsError> {
        Ok(Hash::hash(&self.to_bytes_compact(&context)?))
    }
}

impl SerializeCompact for Operation {
    fn to_bytes_compact(&self, context: &SerializationContext) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        res.extend(&self.signature.to_bytes());

        // operation content
        res.extend(&self.content.to_bytes_compact(&context)?);

        Ok(res)
    }
}

impl DeserializeCompact for Operation {
    fn from_bytes_compact(
        buffer: &[u8],
        context: &SerializationContext,
    ) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0;

        // signature
        let signature = Signature::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        cursor += SIGNATURE_SIZE_BYTES;

        let (content, delta) = OperationContent::from_bytes_compact(&buffer[cursor..], &context)?;
        cursor += delta;

        Ok((Operation { content, signature }, cursor))
    }
}

#[cfg(test)]
mod tests {
    use crypto::signature::SignatureEngine;

    use super::*;

    #[test]
    fn test_operation_type() {
        let context = SerializationContext {
            max_block_size: 100000,
            max_block_operations: 1000000,
            parent_count: 2,
            max_peer_list_length: 128,
            max_message_size: 3 * 1024 * 1024,
            max_bootstrap_blocks: 100,
            max_bootstrap_cliques: 100,
            max_bootstrap_deps: 100,
            max_bootstrap_children: 100,
            max_ask_blocks_per_message: 10,
            max_bootstrap_message_size: 100000000,
        };
        let secp = SignatureEngine::new();
        let sender_priv = SignatureEngine::generate_random_private_key();
        let sender_pub = secp.derive_public_key(&sender_priv);

        let recv_priv = SignatureEngine::generate_random_private_key();
        let recv_pub = secp.derive_public_key(&recv_priv);

        let op = OperationType::Transaction {
            recipient_address: Address::new(recv_pub),
            amount: 0,
        };
        let content = OperationContent {
            fee: 20,
            expiration_period: 50,
            creator_public_key: sender_pub,
            op,
        };
        let hash = content.compute_hash(&context).unwrap();
        let signature = secp.sign(&hash, &sender_priv).unwrap();

        let op = Operation {
            content: content.clone(),
            signature,
        };

        let ser_content = content.to_bytes_compact(&context).unwrap();
        let (res_content, _) =
            OperationContent::from_bytes_compact(&ser_content, &context).unwrap();
        assert_eq!(res_content, content);

        let ser_op = op.to_bytes_compact(&context).unwrap();
        let (res_op, _) = Operation::from_bytes_compact(&ser_op, &context).unwrap();
        assert_eq!(res_op, op);
    }
}
