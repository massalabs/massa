// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::constants::{ADDRESS_SIZE_BYTES, OPERATION_ID_SIZE_BYTES};
use crate::prehash::{BuildMap, PreHashed, Set};
use crate::signed::{Id, Signable, Signed};
use crate::with_serialization_context;
use crate::{
    serialization::{
        array_from_slice, DeserializeCompact, DeserializeVarInt, SerializeCompact, SerializeVarInt,
    },
    Address, Amount, ModelsError,
};
use massa_hash::Hash;
use massa_signature::{PublicKey, PUBLIC_KEY_SIZE_BYTES};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use serde::{Deserialize, Serialize};
use std::convert::TryInto;
use std::fmt::Formatter;
use std::{ops::RangeInclusive, str::FromStr};

const OPERATION_ID_STRING_PREFIX: &str = "OPE";

/// operation id
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
    /// op id into bytes
    pub fn to_bytes(&self) -> [u8; OPERATION_ID_SIZE_BYTES] {
        self.0.to_bytes()
    }

    /// op id into bytes
    pub fn into_bytes(self) -> [u8; OPERATION_ID_SIZE_BYTES] {
        self.0.into_bytes()
    }

    /// op id from bytes
    pub fn from_bytes(data: &[u8; OPERATION_ID_SIZE_BYTES]) -> Result<OperationId, ModelsError> {
        Ok(OperationId(
            Hash::from_bytes(data).map_err(|_| ModelsError::HashError)?,
        ))
    }

    /// op id from `bs58` check
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
    CallSC = 4,
}

/// the operation as sent in the network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    /// the operation creator public key
    pub sender_public_key: PublicKey,
    /// the fee they have decided for this operation
    pub fee: Amount,
    /// after `expire_period` slot the operation won't be included in a block
    pub expire_period: u64,
    /// the type specific operation part
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

impl Signable<OperationId> for Operation {}

/// signed operation
pub type SignedOperation = Signed<Operation, OperationId>;

/// Type specific operation content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationType {
    /// transfer coins from sender to recipient
    Transaction {
        /// recipient address
        recipient_address: Address,
        /// amount
        amount: Amount,
    },
    /// the sender buys `roll_count` rolls. Roll price is defined in configuration
    RollBuy {
        /// roll count
        roll_count: u64,
    },
    /// the sender sells `roll_count` rolls. Roll price is defined in configuration
    RollSell {
        /// roll count
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
    /// Calls an exported function from a stored smart contract
    CallSC {
        /// Target smart contract address
        target_addr: Address,
        /// Target function name. No function is called if empty.
        target_func: String,
        /// Parameter to pass to the target function
        param: String,
        /// The maximum amount of gas that the execution of the contract is allowed to cost.
        max_gas: u64,
        /// Extra coins that are spent from the caller's sequential balance and transferred to the target
        sequential_coins: Amount,
        /// Extra coins that are spent from the caller's parallel balance and transferred to the target
        parallel_coins: Amount,
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
                writeln!(f, "\t- Roll count:{}", roll_count)?;
            }
            OperationType::RollSell { roll_count } => {
                writeln!(f, "Sell rolls:")?;
                writeln!(f, "\t- Roll count:{}", roll_count)?;
            }
            OperationType::ExecuteSC {
                max_gas,
                coins,
                gas_price,
                ..
                // data, // this field is ignored because bytes eh
            } => {
                writeln!(f, "ExecuteSC: ")?;
                writeln!(f, "\t- max_gas:{}", max_gas)?;
                writeln!(f, "\t- gas_price:{}", gas_price)?;
                writeln!(f, "\t- coins:{}", coins)?;
            },
            OperationType::CallSC {
                max_gas,
                parallel_coins,
                sequential_coins,
                gas_price,
                target_addr,
                target_func,
                param
            } => {
                writeln!(f, "CallSC:")?;
                writeln!(f, "\t- target address:{}", target_addr)?;
                writeln!(f, "\t- target function:{}", target_func)?;
                writeln!(f, "\t- target parameter:{}", param)?;
                writeln!(f, "\t- max_gas:{}", max_gas)?;
                writeln!(f, "\t- gas_price:{}", gas_price)?;
                writeln!(f, "\t- sequential coins:{}", sequential_coins)?;
                writeln!(f, "\t- parallel coins:{}", parallel_coins)?;
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
            OperationType::CallSC {
                max_gas,
                parallel_coins,
                sequential_coins,
                gas_price,
                target_addr,
                target_func,
                param,
            } => {
                // type id
                res.extend(u32::from(OperationTypeId::CallSC).to_varint_bytes());

                // Max gas.
                res.extend(max_gas.to_varint_bytes());

                // Parallel coins
                res.extend(&parallel_coins.to_bytes_compact()?);

                // Sequential coins
                res.extend(&sequential_coins.to_bytes_compact()?);

                // Gas price.
                res.extend(&gas_price.to_bytes_compact()?);

                // Target address
                res.extend(target_addr.to_bytes());

                // Target function name
                let func_name_bytes = target_func.as_bytes();
                let func_name_len: u8 = func_name_bytes.len().try_into().map_err(|_| {
                    ModelsError::SerializeError(
                        "CallSC target function name length does not fit in u8".into(),
                    )
                })?;
                res.push(func_name_len);
                res.extend(func_name_bytes);

                // Parameter
                let param_bytes = param.as_bytes();
                let param_len: u16 = param_bytes.len().try_into().map_err(|_| {
                    ModelsError::SerializeError(
                        "CallSC parameter length does not fit in u16".into(),
                    )
                })?;
                res.extend(param_len.to_varint_bytes());
                res.extend(param_bytes);
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
            OperationTypeId::CallSC => {
                // Max gas.
                let (max_gas, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
                cursor += delta;

                // Parallel coins
                let (parallel_coins, delta) = Amount::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                // Sequential coins
                let (sequential_coins, delta) = Amount::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                // Gas price.
                let (gas_price, delta) = Amount::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;

                // Target address
                let target_addr = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += ADDRESS_SIZE_BYTES;

                // Target function name
                let func_name_len = *buffer
                    .get(cursor)
                    .ok_or_else(|| ModelsError::DeserializeError("buffer too small".into()))?;
                cursor += 1;
                let target_func = match buffer.get(cursor..(cursor + func_name_len as usize)) {
                    Some(s) => {
                        cursor += s.len();
                        String::from_utf8(s.to_vec()).map_err(|_| {
                            ModelsError::DeserializeError("string is not utf8".into())
                        })?
                    }
                    None => {
                        return Err(ModelsError::DeserializeError("buffer too small".into()));
                    }
                };

                // parameter
                let (param_len, delta) = u16::from_varint_bytes(&buffer[cursor..])?;
                cursor += delta;
                let param = match buffer.get(cursor..(cursor + param_len as usize)) {
                    Some(s) => {
                        cursor += s.len();
                        String::from_utf8(s.to_vec()).map_err(|_| {
                            ModelsError::DeserializeError("string is not utf8".into())
                        })?
                    }
                    None => {
                        return Err(ModelsError::DeserializeError("buffer too small".into()));
                    }
                };

                OperationType::CallSC {
                    target_addr,
                    sequential_coins,
                    parallel_coins,
                    target_func,
                    max_gas,
                    gas_price,
                    param,
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

impl SignedOperation {
    /// Verifies the signature and integrity of the operation and computes operation ID
    pub fn verify_integrity(&self) -> Result<OperationId, ModelsError> {
        self.verify_signature(&self.content.sender_public_key)?;
        self.content.compute_id()
    }
}

impl Operation {
    /// get the range of periods during which an operation is valid
    pub fn get_validity_range(&self, operation_validity_period: u64) -> RangeInclusive<u64> {
        let start = self.expire_period.saturating_sub(operation_validity_period);
        start..=self.expire_period
    }

    /// Get the amount of gas used by the operation
    pub fn get_gas_usage(&self) -> u64 {
        match &self.op {
            OperationType::ExecuteSC { max_gas, .. } => *max_gas,
            OperationType::CallSC { max_gas, .. } => *max_gas,
            OperationType::RollBuy { .. } => 0,
            OperationType::RollSell { .. } => 0,
            OperationType::Transaction { .. } => 0,
        }
    }

    /// Get the amount of coins used by the operation to pay for gas
    pub fn get_gas_coins(&self) -> Amount {
        match &self.op {
            OperationType::ExecuteSC {
                max_gas, gas_price, ..
            } => gas_price.saturating_mul_u64(*max_gas),
            OperationType::CallSC {
                max_gas, gas_price, ..
            } => gas_price.saturating_mul_u64(*max_gas),
            OperationType::RollBuy { .. } => Amount::default(),
            OperationType::RollSell { .. } => Amount::default(),
            OperationType::Transaction { .. } => Amount::default(),
        }
    }

    /// get the addresses that are involved in this operation from a ledger point of view
    pub fn get_ledger_involved_addresses(&self) -> Result<Set<Address>, ModelsError> {
        let mut res = Set::<Address>::default();
        let emitter_address = Address::from_public_key(&self.sender_public_key);
        res.insert(emitter_address);
        match &self.op {
            OperationType::Transaction {
                recipient_address, ..
            } => {
                res.insert(*recipient_address);
            }
            OperationType::RollBuy { .. } => {}
            OperationType::RollSell { .. } => {}
            OperationType::ExecuteSC { .. } => {}
            OperationType::CallSC { target_addr, .. } => {
                res.insert(*target_addr);
            }
        }
        Ok(res)
    }

    /// get the addresses that are involved in this operation from a rolls point of view
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
            OperationType::CallSC { .. } => {}
        }
        Ok(res)
    }
}

/// Set of operation ids
pub type OperationIds = Set<OperationId>;

impl SerializeCompact for OperationIds {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let list_len: u32 = self.len().try_into().map_err(|_| {
            ModelsError::SerializeError("could not encode AskForBlocks list length as u32".into())
        })?;
        let mut res = Vec::new();
        res.extend(list_len.to_varint_bytes());
        for hash in self {
            res.extend(&hash.to_bytes());
        }
        Ok(res)
    }
}

/// Deserialize from the given `buffer`.
///
/// You know that the maximum number of ids is `max_operations_per_message` taken
/// from the node configuration.
///
/// # Return
/// A result that return the deserialized `Vec<OperationId>`
impl DeserializeCompact for OperationIds {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let max_operations_per_message =
            with_serialization_context(|context| context.max_operations_per_message);
        let mut cursor = 0usize;
        let (length, delta) =
            u32::from_varint_bytes_bounded(&buffer[cursor..], max_operations_per_message)?;
        cursor += delta;
        // hash list
        let mut list: OperationIds =
            Set::with_capacity_and_hasher(length as usize, BuildMap::default());
        for _ in 0..length {
            let b_id = OperationId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += OPERATION_ID_SIZE_BYTES;
            list.insert(b_id);
        }
        Ok((list, cursor))
    }
}

/// Set of self containing signed operations.
pub type Operations = Vec<SignedOperation>;

impl SerializeCompact for Operations {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res = Vec::new();
        res.extend((self.len() as u32).to_varint_bytes());
        for op in self.iter() {
            res.extend(op.to_bytes_compact()?);
        }
        Ok(res)
    }
}

/// Deserialize from the given `buffer`.
///
/// You know that the maximum number of ids is `max_operations_per_message` taken
/// from the node configuration.
///
/// # Return
/// A result that return the deserialized `Vec<Operation>`
impl DeserializeCompact for Operations {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let max_operations_per_message =
            with_serialization_context(|context| context.max_operations_per_message);
        let mut cursor = 0usize;
        let (length, delta) =
            u32::from_varint_bytes_bounded(&buffer[cursor..], max_operations_per_message)?;
        cursor += delta;
        let mut ops: Operations = Operations::with_capacity(length as usize);
        for _ in 0..length {
            let (operation, delta) = SignedOperation::from_bytes_compact(&buffer[cursor..])?;
            cursor += delta;
            ops.push(operation);
        }
        Ok((ops, cursor))
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

    #[test]
    #[serial]
    fn test_callsc() {
        let sender_priv = generate_random_private_key();
        let sender_pub = derive_public_key(&sender_priv);

        let target_priv = generate_random_private_key();
        let target_pub = derive_public_key(&target_priv);
        let target_addr = Address::from_public_key(&target_pub);

        let op = OperationType::CallSC {
            max_gas: 123,
            target_addr,
            parallel_coins: Amount::from_str("456.789").unwrap(),
            sequential_coins: Amount::from_str("123.111").unwrap(),
            gas_price: Amount::from_str("772.122").unwrap(),
            target_func: "target function".to_string(),
            param: "parameter".to_string(),
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
