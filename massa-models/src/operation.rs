// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::constants::{ADDRESS_SIZE_BYTES, OPERATION_ID_SIZE_BYTES};
use crate::prehash::{PreHashed, Set};
use crate::signed::{Id, Signable, Signed};
use crate::{
    serialization::{
        array_from_slice, DeserializeCompact, DeserializeVarInt, SerializeCompact, SerializeVarInt,
    },
    Address, Amount, ModelsError,
};
use massa_hash::hash::Hash;
use massa_signature::{PublicKey, PUBLIC_KEY_SIZE_BYTES};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use serde::{Deserialize, Serialize};
use std::convert::TryInto;
use std::fmt::Formatter;
use std::{ops::RangeInclusive, str::FromStr};

const OPERATION_ID_STRING_PREFIX: &str = "OPE";
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct OperationId(Hash);

impl std::fmt::Display for OperationId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if cfg!(feature = "hash-prefix") {
            write!(
                f,
                "{}-{}",
                OPERATION_ID_STRING_PREFIX,
                self.0.to_bs58_check()
            )
        } else {
            write!(f, "{}", self.0.to_bs58_check())
        }
    }
}

impl std::fmt::Debug for OperationId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if cfg!(feature = "hash-prefix") {
            write!(
                f,
                "{}-{}",
                OPERATION_ID_STRING_PREFIX,
                self.0.to_bs58_check()
            )
        } else {
            write!(f, "{}", self.0.to_bs58_check())
        }
    }
}

impl FromStr for OperationId {
    type Err = ModelsError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if cfg!(feature = "hash-prefix") {
            let v: Vec<_> = s.split('-').collect();
            if v.len() != 2 {
                // assume there is no prefix
                Ok(OperationId(Hash::from_str(s)?))
            } else if v[0] != OPERATION_ID_STRING_PREFIX {
                Err(ModelsError::WrongPrefix(
                    OPERATION_ID_STRING_PREFIX.to_string(),
                    v[0].to_string(),
                ))
            } else {
                Ok(OperationId(Hash::from_str(v[1])?))
            }
        } else {
            Ok(OperationId(Hash::from_str(s)?))
        }
    }
}

impl PreHashed for OperationId {}
impl Id for OperationId {
    fn new(hash: Hash) -> Self {
        OperationId(hash)
    }
}

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
    ExecuteSC = 3,
}

// #[derive(Debug, Clone, Serialize, Deserialize)]
// pub struct Operation {
//     pub content: Operation,
//     pub signature: Signature,
// }

// impl std::fmt::Display for Operation {
//     fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
//         writeln!(
//             f,
//             "Id: {}",
//             match self.content.compute_id() {
//                 Ok(id) => format!("{}", id),
//                 Err(e) => format!("error computing id: {}", e),
//             }
//         )?;
//         writeln!(f, "Signature: {}", self.signature)?;
//         let addr = Address::from_public_key(&self.content.sender_public_key);
//         let amount = self.content.fee.to_string();
//         writeln!(
//             f,
//             "sender: {}     fee: {}     expire_period: {}",
//             addr, amount, self.content.expire_period,
//         )?;
//         writeln!(f, "{}", self.content.op)?;
//         Ok(())
//     }
// }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    pub sender_public_key: PublicKey,
    pub fee: Amount,
    pub expire_period: u64,
    pub op: OperationType,
}

impl std::fmt::Display for Operation {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Sender public key: {}", self.sender_public_key)?;
        writeln!(f, "Fee: {}", self.fee)?;
        writeln!(f, "Expire period: {}", self.expire_period)?;
        writeln!(f, "Operation type: {}", self.op)?;
        Ok(())
    }
}

impl Signable<OperationId> for Operation {
    fn get_signature_message(&self) -> Result<Hash, ModelsError> {
        Ok(Hash::compute_from(&self.to_bytes_compact()?))
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
    /// Execute a smart contract.
    ExecuteSC {
        /// Smart contract bytecode.
        data: Vec<u8>,
        /// The maximum amount of gas that the execution of the contract is allowed to cost.
        max_gas: u64,
        /// Extra coins that are spent by consensus and are available in the execution context of the contract.
        coins: Amount,
        /// The price per unit of gas that the caller is willing to pay for the execution.
        gas_price: Amount,
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
            OperationType::ExecuteSC {
                data: _,
                max_gas: _,
                coins: _,
                gas_price: _,
            } => {
                writeln!(f, "ExecuteSC")?;
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
            OperationType::ExecuteSC {
                data,
                max_gas,
                coins,
                gas_price,
            } => {
                // type id
                res.extend(u32::from(OperationTypeId::ExecuteSC).to_varint_bytes());

                // Max gas.
                res.extend(max_gas.to_varint_bytes());

                // Coins.
                res.extend(&coins.to_bytes_compact()?);

                // Gas price.
                res.extend(&gas_price.to_bytes_compact()?);

                // Contract data length
                let data_len: u32 = data.len().try_into().map_err(|_| {
                    ModelsError::SerializeError("ExecuteSC data length does not fit in u32".into())
                })?;
                res.extend(&data_len.to_varint_bytes());

                // Contract data
                res.extend(data);
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
            OperationTypeId::ExecuteSC => {
                // Max gas.
                let (max_gas, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
                cursor += delta;

                // Coins.
                let (coins, delta) = Amount::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                // Gas price.
                let (gas_price, delta) = Amount::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                // Contract data length
                let (data_len, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
                // TODO limit size
                cursor += delta;

                // Contract data.
                let mut data = Vec::with_capacity(data_len as usize);
                if buffer[cursor..].len() < data_len as usize {
                    return Err(ModelsError::DeserializeError(
                        "ExecuteSC deserialization failed: not enough buffer to get all data"
                            .into(),
                    ));
                }
                data.extend(&buffer[cursor..(cursor + (data_len as usize))]);
                cursor += data_len as usize;

                OperationType::ExecuteSC {
                    data,
                    max_gas,
                    coins,
                    gas_price,
                }
            }
        };
        Ok((res, cursor))
    }
}

/// Checks performed:
/// - Validity of the fee.
/// - Validity of the operation type.
impl SerializeCompact for Operation {
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
impl DeserializeCompact for Operation {
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
            Operation {
                fee,
                expire_period,
                sender_public_key,
                op,
            },
            cursor,
        ))
    }
}

impl Signed<Operation, OperationId> {
    /// Verifies the signature and integrity of the operation and computes operation ID
    pub fn verify_integrity(&self) -> Result<OperationId, ModelsError> {
        self.verify_signature(&self.content.sender_public_key)?;
        self.content.compute_id()
    }
}

impl Operation {
    pub fn get_validity_range(&self, operation_validity_period: u64) -> RangeInclusive<u64> {
        let start = self.expire_period.saturating_sub(operation_validity_period);
        start..=self.expire_period
    }

    pub fn get_ledger_involved_addresses(&self) -> Result<Set<Address>, ModelsError> {
        let mut res = Set::<Address>::default();
        let emitter_address = Address::from_public_key(&self.sender_public_key);
        res.insert(emitter_address);
        match self.op {
            OperationType::Transaction {
                recipient_address, ..
            } => {
                res.insert(recipient_address);
            }
            OperationType::RollBuy { .. } => {}
            OperationType::RollSell { .. } => {}
            OperationType::ExecuteSC { .. } => {}
        }
        Ok(res)
    }

    pub fn get_roll_involved_addresses(&self) -> Result<Set<Address>, ModelsError> {
        let mut res = Set::<Address>::default();
        match self.op {
            OperationType::Transaction { .. } => {}
            OperationType::RollBuy { .. } => {
                res.insert(Address::from_public_key(&self.sender_public_key));
            }
            OperationType::RollSell { .. } => {
                res.insert(Address::from_public_key(&self.sender_public_key));
            }
            OperationType::ExecuteSC { .. } => {}
        }
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use massa_signature::{derive_public_key, generate_random_private_key};
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_transaction() {
        let sender_priv = generate_random_private_key();
        let sender_pub = derive_public_key(&sender_priv);

        let recv_priv = generate_random_private_key();
        let recv_pub = derive_public_key(&recv_priv);

        let op = OperationType::Transaction {
            recipient_address: Address::from_public_key(&recv_pub),
            amount: Amount::default(),
        };
        let ser_type = op.to_bytes_compact().unwrap();
        let (res_type, _) = OperationType::from_bytes_compact(&ser_type).unwrap();
        assert_eq!(format!("{}", res_type), format!("{}", op));

        let content = Operation {
            fee: Amount::from_str("20").unwrap(),
            sender_public_key: sender_pub,
            op,
            expire_period: 50,
        };

        let ser_content = content.to_bytes_compact().unwrap();
        let (res_content, _) = Operation::from_bytes_compact(&ser_content).unwrap();
        assert_eq!(format!("{}", res_content), format!("{}", content));

        let op = Signed::new_signed(content, &sender_priv).unwrap().1;

        let ser_op = op.to_bytes_compact().unwrap();
        let (res_op, _) = Signed::<Operation, OperationId>::from_bytes_compact(&ser_op).unwrap();
        assert_eq!(format!("{}", res_op), format!("{}", op));

        assert_eq!(op.content.get_validity_range(10), 40..=50);
    }

    #[test]
    #[serial]
    fn test_executesc() {
        let sender_priv = generate_random_private_key();
        let sender_pub = derive_public_key(&sender_priv);

        let op = OperationType::ExecuteSC {
            max_gas: 123,
            coins: Amount::from_str("456.789").unwrap(),
            gas_price: Amount::from_str("772.122").unwrap(),
            data: vec![23u8, 123u8, 44u8],
        };
        let ser_type = op.to_bytes_compact().unwrap();
        let (res_type, _) = OperationType::from_bytes_compact(&ser_type).unwrap();
        assert_eq!(format!("{}", res_type), format!("{}", op));

        let content = Operation {
            fee: Amount::from_str("20").unwrap(),
            sender_public_key: sender_pub,
            op,
            expire_period: 50,
        };

        let ser_content = content.to_bytes_compact().unwrap();
        let (res_content, _) = Operation::from_bytes_compact(&ser_content).unwrap();
        assert_eq!(format!("{}", res_content), format!("{}", content));

        let op = Signed::new_signed(content, &sender_priv).unwrap().1;

        let ser_op = op.to_bytes_compact().unwrap();
        let (res_op, _) = Signed::<Operation, OperationId>::from_bytes_compact(&ser_op).unwrap();
        assert_eq!(format!("{}", res_op), format!("{}", op));

        assert_eq!(op.content.get_validity_range(10), 40..=50);
    }
}
