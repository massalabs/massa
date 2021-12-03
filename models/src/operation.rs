// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::{
    address::AddressHashSet,
    hhasher::{HHashMap, HHashSet, PreHashed},
    serialization::{
        array_from_slice, DeserializeCompact, DeserializeVarInt, SerializeCompact, SerializeVarInt,
    },
    Address, Amount, ModelsError, ADDRESS_SIZE_BYTES,
};
use massa_hash::hash::{Hash, HASH_SIZE_BYTES};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use serde::{Deserialize, Serialize};
use signature::{
    verify_signature, PublicKey, Signature, PUBLIC_KEY_SIZE_BYTES, SIGNATURE_SIZE_BYTES,
};
use std::convert::TryInto;
use std::fmt::Formatter;
use std::{ops::RangeInclusive, str::FromStr};

pub const OPERATION_ID_SIZE_BYTES: usize = HASH_SIZE_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct OperationId(Hash);

pub type OperationHashMap<T> = HHashMap<OperationId, T>;
pub type OperationHashSet = HHashSet<OperationId>;

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

impl PreHashed for OperationId {}

impl OperationId {
    pub fn to_bytes(&self) -> [u8; OPERATION_ID_SIZE_BYTES] {
        self.0.to_bytes()
    }

    pub fn into_bytes(self) -> [u8; OPERATION_ID_SIZE_BYTES] {
        self.0.into_bytes()
    }

    pub fn from_bytes(data: &[u8; OPERATION_ID_SIZE_BYTES]) -> Result<OperationId, ModelsError> {
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
    RollBuy = 1,
    RollSell = 2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    pub content: OperationContent,
    pub signature: Signature,
}

impl std::fmt::Display for Operation {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Id: {}",
            match self.get_operation_id() {
                Ok(id) => format!("{}", id),
                Err(e) => format!("error computing id: {}", e),
            }
        )?;
        writeln!(f, "Signature: {}", self.signature)?;
        let addr = Address::from_public_key(&self.content.sender_public_key)
            .map_err(|_| std::fmt::Error)?;
        let amount = self.content.fee.to_string();
        writeln!(
            f,
            "sender: {}     fee: {}     expire_period: {}",
            addr, amount, self.content.expire_period,
        )?;
        writeln!(f, "{}", self.content.op)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationContent {
    pub sender_public_key: PublicKey,
    pub fee: Amount,
    pub expire_period: u64,
    pub op: OperationType,
}

impl std::fmt::Display for OperationContent {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Sender public key: {}", self.sender_public_key)?;
        writeln!(f, "Fee: {}", self.fee)?;
        writeln!(f, "Expire period: {}", self.expire_period)?;
        writeln!(f, "Operation type: {}", self.op)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationType {
    Transaction {
        recipient_address: Address,
        amount: Amount,
    },
    RollBuy {
        roll_count: u64,
    },
    RollSell {
        roll_count: u64,
    },
}

impl std::fmt::Display for OperationType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            OperationType::Transaction {
                recipient_address,
                amount,
            } => {
                writeln!(f, "Transaction:")?;
                writeln!(f, "\t- Recipient:{}", recipient_address)?;
                writeln!(f, "\t  Amount:{}", amount)?;
            }
            OperationType::RollBuy { roll_count } => {
                writeln!(f, "Buy rolls:")?;
                write!(f, "\t- Roll count:{}", roll_count)?;
            }
            OperationType::RollSell { roll_count } => {
                writeln!(f, "Sell rolls:")?;
                write!(f, "\t- Roll count:{}", roll_count)?;
            }
        }
        Ok(())
    }
}

/// Checks performed:
/// - Validity of the amount.
impl SerializeCompact for OperationType {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
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
                res.extend(&amount.to_bytes_compact()?);
            }
            OperationType::RollBuy { roll_count } => {
                // type id
                res.extend(u32::from(OperationTypeId::RollBuy).to_varint_bytes());

                // roll_count
                res.extend(&roll_count.to_varint_bytes());
            }
            OperationType::RollSell { roll_count } => {
                // type id
                res.extend(u32::from(OperationTypeId::RollSell).to_varint_bytes());

                // roll_count
                res.extend(&roll_count.to_varint_bytes());
            }
        }
        Ok(res)
    }
}

/// Checks performed:
/// - Validity of the type id.
/// - Validity of the address(for transactions).
/// - Validity of the amount(for transactions).
/// - Validity of the roll count(for roll buy/sell).
impl DeserializeCompact for OperationType {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
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
                let (amount, delta) = Amount::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                OperationType::Transaction {
                    recipient_address,
                    amount,
                }
            }
            OperationTypeId::RollBuy => {
                // roll_count
                let (roll_count, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
                cursor += delta;

                OperationType::RollBuy { roll_count }
            }
            OperationTypeId::RollSell => {
                // roll_count
                let (roll_count, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
                cursor += delta;

                OperationType::RollSell { roll_count }
            }
        };
        Ok((res, cursor))
    }
}

/// Checks performed:
/// - Validity of the fee.
/// - Validity of the operation type.
impl SerializeCompact for OperationContent {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // fee
        res.extend(&self.fee.to_bytes_compact()?);

        // expire period
        res.extend(self.expire_period.to_varint_bytes());

        // sender public key
        res.extend(&self.sender_public_key.to_bytes());

        // operation type
        res.extend(&self.op.to_bytes_compact()?);

        Ok(res)
    }
}

/// Checks performed:
/// - Validity of the fee.
/// - Validity of the expire period.
/// - Validity of the sender public key.
/// - Validity of the operation type.
impl DeserializeCompact for OperationContent {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0usize;

        // fee
        let (fee, delta) = Amount::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        // expire period
        let (expire_period, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        // sender public key
        let sender_public_key = PublicKey::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        cursor += PUBLIC_KEY_SIZE_BYTES;

        // op
        let (op, delta) = OperationType::from_bytes_compact(&buffer[cursor..])?;
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
    /// Verifies the signature and integrity of the operation and computes operation ID
    pub fn verify_integrity(&self) -> Result<OperationId, ModelsError> {
        self.verify_signature()?;
        self.get_operation_id()
    }

    pub fn verify_signature(&self) -> Result<(), ModelsError> {
        let content_hash = Hash::from(&self.content.to_bytes_compact()?);
        verify_signature(
            &content_hash,
            &self.signature,
            &self.content.sender_public_key,
        )?;
        Ok(())
    }

    pub fn get_operation_id(&self) -> Result<OperationId, ModelsError> {
        Ok(OperationId(Hash::from(&self.to_bytes_compact()?)))
    }

    pub fn get_validity_range(&self, operation_validity_period: u64) -> RangeInclusive<u64> {
        let start = self
            .content
            .expire_period
            .saturating_sub(operation_validity_period);
        start..=self.content.expire_period
    }

    pub fn get_ledger_involved_addresses(&self) -> Result<AddressHashSet, ModelsError> {
        let mut res = AddressHashSet::default();
        let emitter_address = Address::from_public_key(&self.content.sender_public_key)?;
        res.insert(emitter_address);
        match self.content.op {
            OperationType::Transaction {
                recipient_address, ..
            } => {
                res.insert(recipient_address);
            }
            OperationType::RollBuy { .. } => {}
            OperationType::RollSell { .. } => {}
        }
        Ok(res)
    }

    pub fn get_roll_involved_addresses(&self) -> Result<AddressHashSet, ModelsError> {
        let mut res = AddressHashSet::default();
        match self.content.op {
            OperationType::Transaction { .. } => {}
            OperationType::RollBuy { .. } => {
                res.insert(Address::from_public_key(&self.content.sender_public_key)?);
            }
            OperationType::RollSell { .. } => {
                res.insert(Address::from_public_key(&self.content.sender_public_key)?);
            }
        }
        Ok(res)
    }
}

/// Checks performed:
/// - Content.
impl SerializeCompact for Operation {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // content
        res.extend(self.content.to_bytes_compact()?);

        // signature
        res.extend(&self.signature.to_bytes());

        Ok(res)
    }
}

/// Checks performed:
/// - Content.
/// - Signature.
impl DeserializeCompact for Operation {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0;

        // content
        let (content, delta) = OperationContent::from_bytes_compact(&buffer[cursor..])?;
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
    use serial_test::serial;
    use signature::{derive_public_key, generate_random_private_key, sign};

    #[test]
    #[serial]
    fn test_operation_type() {
        let sender_priv = generate_random_private_key();
        let sender_pub = derive_public_key(&sender_priv);

        let recv_priv = generate_random_private_key();
        let recv_pub = derive_public_key(&recv_priv);

        let op = OperationType::Transaction {
            recipient_address: Address::from_public_key(&recv_pub).unwrap(),
            amount: Amount::default(),
        };
        let ser_type = op.to_bytes_compact().unwrap();
        let (res_type, _) = OperationType::from_bytes_compact(&ser_type).unwrap();
        assert_eq!(format!("{}", res_type), format!("{}", op));

        let content = OperationContent {
            fee: Amount::from_str("20").unwrap(),
            sender_public_key: sender_pub,
            op,
            expire_period: 50,
        };

        let ser_content = content.to_bytes_compact().unwrap();
        let (res_content, _) = OperationContent::from_bytes_compact(&ser_content).unwrap();
        assert_eq!(format!("{}", res_content), format!("{}", content));

        let hash = Hash::from(&content.to_bytes_compact().unwrap());
        let signature = sign(&hash, &sender_priv).unwrap();

        let op = Operation { content, signature };

        let ser_op = op.to_bytes_compact().unwrap();
        let (res_op, _) = Operation::from_bytes_compact(&ser_op).unwrap();
        assert_eq!(format!("{}", res_op), format!("{}", op));

        assert_eq!(op.get_validity_range(10), 40..=50);
    }
}
