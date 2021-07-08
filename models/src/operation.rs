use crate::{
    serialization::{
        array_from_slice, DeserializeCompact, DeserializeVarInt, SerializeCompact, SerializeVarInt,
    },
    ModelsError, SerializationContext,
};
use crypto::{
    hash::{Hash, HASH_SIZE_BYTES},
    signature::{PublicKey, Signature, PUBLIC_KEY_SIZE_BYTES},
};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use serde::{Deserialize, Serialize};
use std::convert::TryInto;

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u32)]
enum OperationTypeId {
    Transaction = 0,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Operation {
    Transaction {
        content: TransactionContent,
        signature: Signature,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionContent {
    pub expire_period: u64,
    pub sender_public_key: PublicKey,
    pub recipient_address: Hash, // Hash of the recipient's public key
    pub amount: u64,
    pub fee: u64,
}

impl SerializeCompact for TransactionContent {
    fn to_bytes_compact(&self, _context: &SerializationContext) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // expire period
        res.extend(self.expire_period.to_varint_bytes());

        // sender public key
        res.extend(&self.sender_public_key.to_bytes());

        // recipient address
        res.extend(&self.recipient_address.to_bytes());

        // amount
        res.extend(self.amount.to_varint_bytes());

        // fee
        res.extend(self.fee.to_varint_bytes());

        Ok(res)
    }
}

impl DeserializeCompact for TransactionContent {
    fn from_bytes_compact(
        buffer: &[u8],
        _context: &SerializationContext,
    ) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0usize;

        // expire period
        let (expire_period, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        // sender public key
        let sender_public_key = PublicKey::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        cursor += PUBLIC_KEY_SIZE_BYTES;

        // recipient address
        let recipient_address = Hash::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        cursor += HASH_SIZE_BYTES;

        // amount
        let (amount, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        // fee
        let (fee, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        Ok((
            TransactionContent {
                expire_period,
                sender_public_key,
                recipient_address,
                amount,
                fee,
            },
            cursor,
        ))
    }
}
impl TransactionContent {
    pub fn compute_hash(&self, context: &SerializationContext) -> Result<Hash, ModelsError> {
        Ok(Hash::hash(&self.to_bytes_compact(&context)?))
    }
}

impl SerializeCompact for Operation {
    fn to_bytes_compact(&self, context: &SerializationContext) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();
        match self {
            Operation::Transaction { content, signature } => {
                res.extend(u32::from(OperationTypeId::Transaction).to_varint_bytes());
                res.extend(content.to_bytes_compact(&context)?);
                res.extend(&signature.to_bytes());
            }
        }
        Ok(res)
    }
}

impl DeserializeCompact for Operation {
    fn from_bytes_compact(
        buffer: &[u8],
        context: &SerializationContext,
    ) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0;

        // type id
        let (type_id_raw, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        let type_id: OperationTypeId = type_id_raw
            .try_into()
            .map_err(|_| ModelsError::DeserializeError("invalid message type ID".into()))?;

        let res = match type_id {
            OperationTypeId::Transaction => {
                // Transaction
                let (content, delta) =
                    TransactionContent::from_bytes_compact(&buffer[cursor..], &context)?;
                cursor += delta;
                let signature = Signature::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += delta;
                Operation::Transaction { content, signature }
            }
        };
        Ok((res, cursor))
    }
}
