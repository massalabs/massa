use crate::{
    serialization::{
        array_from_slice, DeserializeCompact, DeserializeVarInt, SerializeCompact, SerializeVarInt,
    },
    Address, ModelsError, SerializationContext, ADDRESS_SIZE_BYTES,
};
use crypto::{
    hash::{Hash, HASH_SIZE_BYTES},
    signature::{
        verify_signature, PublicKey, Signature, PUBLIC_KEY_SIZE_BYTES, SIGNATURE_SIZE_BYTES,
    },
};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use serde::{Deserialize, Serialize};
use std::{convert::TryInto, ops::Range};
use std::{ops::RangeInclusive, str::FromStr};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct OperationId(Hash);

impl std::fmt::Display for OperationId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0.to_bs58_check())
    }
}

impl FromStr for OperationId {
    type Err = ModelsError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(OperationId(Hash::from_str(s)?))
    }
}

impl OperationId {
    pub fn to_bytes(&self) -> [u8; HASH_SIZE_BYTES] {
        self.0.to_bytes()
    }

    pub fn into_bytes(self) -> [u8; HASH_SIZE_BYTES] {
        self.0.into_bytes()
    }

    pub fn from_bytes(data: &[u8; HASH_SIZE_BYTES]) -> Result<OperationId, ModelsError> {
        Ok(OperationId(
            Hash::from_bytes(data).map_err(|_| ModelsError::HashError)?,
        ))
    }
    pub fn from_bs58_check(data: &str) -> Result<OperationId, ModelsError> {
        Ok(OperationId(
            Hash::from_bs58_check(data).map_err(|_| ModelsError::HashError)?,
        ))
    }
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u32)]
enum OperationTypeId {
    Transaction = 0,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    pub content: OperationContent,
    pub signature: Signature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationContent {
    pub sender_public_key: PublicKey,
    pub fee: u64,
    pub expire_period: u64,
    pub op: OperationType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationType {
    Transaction {
        recipient_address: Address,
        amount: u64,
    },
}

impl SerializeCompact for OperationType {
    fn to_bytes_compact(&self, _context: &SerializationContext) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();
        match self {
            OperationType::Transaction {
                recipient_address,
                amount,
            } => {
                // type id
                res.extend(u32::from(OperationTypeId::Transaction).to_varint_bytes());

                // recipient_address
                res.extend(&recipient_address.to_bytes());

                // amount
                res.extend(&amount.to_varint_bytes());
            }
        }
        Ok(res)
    }
}

impl DeserializeCompact for OperationType {
    fn from_bytes_compact(
        buffer: &[u8],
        _context: &SerializationContext,
    ) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0;

        // type id
        let (type_id_raw, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        let type_id: OperationTypeId = type_id_raw
            .try_into()
            .map_err(|_| ModelsError::DeserializeError("invalid operation type ID".into()))?;

        let res = match type_id {
            OperationTypeId::Transaction => {
                // recipient_address
                let recipient_address = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += ADDRESS_SIZE_BYTES;

                // amount
                let (amount, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
                cursor += delta;

                OperationType::Transaction {
                    recipient_address,
                    amount,
                }
            }
        };
        Ok((res, cursor))
    }
}

impl SerializeCompact for OperationContent {
    fn to_bytes_compact(&self, context: &SerializationContext) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // fee
        res.extend(self.fee.to_varint_bytes());

        // expire period
        res.extend(self.expire_period.to_varint_bytes());

        // sender public key
        res.extend(&self.sender_public_key.to_bytes());

        // operation type
        res.extend(&self.op.to_bytes_compact(context)?);

        Ok(res)
    }
}

impl DeserializeCompact for OperationContent {
    fn from_bytes_compact(
        buffer: &[u8],
        context: &SerializationContext,
    ) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0usize;

        // fee
        let (fee, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        // expire period
        let (expire_period, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        // sender public key
        let sender_public_key = PublicKey::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        cursor += PUBLIC_KEY_SIZE_BYTES;

        // op
        let (op, delta) = OperationType::from_bytes_compact(&buffer[cursor..], context)?;
        cursor += delta;

        Ok((
            OperationContent {
                fee,
                expire_period,
                sender_public_key,
                op,
            },
            cursor,
        ))
    }
}

impl Operation {
    /// Verify the signature and integrity of the operation and computes operation ID
    pub fn verify_integrity(
        &self,
        context: &SerializationContext,
    ) -> Result<OperationId, ModelsError> {
        let content_hash = Hash::hash(&self.content.to_bytes_compact(context)?);
        verify_signature(
            &content_hash,
            &self.signature,
            &self.content.sender_public_key,
        )?;
        self.get_operation_id(context)
    }

    pub fn get_operation_id(
        &self,
        context: &SerializationContext,
    ) -> Result<OperationId, ModelsError> {
        Ok(OperationId(Hash::hash(&self.to_bytes_compact(context)?)))
    }

    pub fn get_validity_range(&self, operation_validity_period: u64) -> RangeInclusive<u64> {
        let start = self
            .content
            .expire_period
            .saturating_sub(operation_validity_period);
        start..=self.content.expire_period
    }
}

impl SerializeCompact for Operation {
    fn to_bytes_compact(&self, context: &SerializationContext) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // content
        res.extend(self.content.to_bytes_compact(&context)?);

        // signature
        res.extend(&self.signature.to_bytes());

        Ok(res)
    }
}

impl DeserializeCompact for Operation {
    fn from_bytes_compact(
        buffer: &[u8],
        context: &SerializationContext,
    ) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0;

        // content
        let (content, delta) = OperationContent::from_bytes_compact(&buffer[cursor..], context)?;
        cursor += delta;

        // signature
        let signature = Signature::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        cursor += SIGNATURE_SIZE_BYTES;

        let res = Operation { content, signature };

        Ok((res, cursor))
    }
}

#[cfg(test)]
mod tests {
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
            max_operations_per_message: 1024,
            max_bootstrap_message_size: 100000000,
        };
        let sender_priv = crypto::generate_random_private_key();
        let sender_pub = crypto::derive_public_key(&sender_priv);

        let recv_priv = crypto::generate_random_private_key();
        let recv_pub = crypto::derive_public_key(&recv_priv);

        let op = OperationType::Transaction {
            recipient_address: Address::from_public_key(&recv_pub).unwrap(),
            amount: 0,
        };
        let ser_type = op.to_bytes_compact(&context).unwrap();
        let (res_type, _) = OperationType::from_bytes_compact(&ser_type, &context).unwrap();
        assert_eq!(format!("{:?}", res_type), format!("{:?}", op));

        let content = OperationContent {
            fee: 20,
            sender_public_key: sender_pub,
            op,
            expire_period: 50,
        };

        let ser_content = content.to_bytes_compact(&context).unwrap();
        let (res_content, _) =
            OperationContent::from_bytes_compact(&ser_content, &context).unwrap();
        assert_eq!(format!("{:?}", res_content), format!("{:?}", content));

        let hash = Hash::hash(&content.to_bytes_compact(&context).unwrap());
        let signature = crypto::sign(&hash, &sender_priv).unwrap();

        let op = Operation {
            content: content.clone(),
            signature,
        };

        let ser_op = op.to_bytes_compact(&context).unwrap();
        let (res_op, _) = Operation::from_bytes_compact(&ser_op, &context).unwrap();
        assert_eq!(format!("{:?}", res_op), format!("{:?}", op));

        assert_eq!(op.get_validity_range(10), 40..=50);
    }
}
