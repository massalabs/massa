// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::constants::OPERATION_ID_SIZE_BYTES;
use crate::node_configuration::MAX_OPERATIONS_PER_MESSAGE;
use crate::node_configuration::OPERATION_ID_PREFIX_SIZE_BYTES;
use crate::prehash::{PreHashed, Set};
use crate::serialization::StringDeserializer;

use crate::wrapped::{Id, Wrapped, WrappedContent, WrappedDeserializer, WrappedSerializer};
use crate::{Address, Amount, ModelsError};
use crate::{
    AddressDeserializer, AmountDeserializer, AmountSerializer, StringSerializer, VecU8Deserializer,
    VecU8Serializer,
};
use massa_hash::{Hash, HashDeserializer};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U16VarIntDeserializer, U16VarIntSerializer,
    U32VarIntDeserializer, U32VarIntSerializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::error::context;
use nom::multi::length_count;
use nom::sequence::tuple;
use nom::AsBytes;
use nom::Parser;
use nom::{
    error::{ContextError, ParseError},
    IResult,
};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use serde::{Deserialize, Serialize};
use std::convert::TryInto;
use std::fmt::Formatter;
use std::{ops::Bound::Included, ops::RangeInclusive, str::FromStr};

const OPERATION_ID_STRING_PREFIX: &str = "OPE";

/// operation id
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct OperationId(Hash);

/// Left part of the operation id hash stored in a vector of size [OPERATION_ID_PREFIX_SIZE_BYTES]
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct OperationPrefixId([u8; OPERATION_ID_PREFIX_SIZE_BYTES]);

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

impl std::fmt::Display for OperationPrefixId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if cfg!(feature = "hash-prefix") {
            write!(
                f,
                "{}-{}",
                OPERATION_ID_STRING_PREFIX,
                bs58::encode(self.0.as_bytes()).into_string()
            )
        } else {
            write!(f, "{}", bs58::encode(self.0.as_bytes()).into_string())
        }
    }
}

impl std::fmt::Debug for OperationPrefixId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if cfg!(feature = "hash-prefix") {
            write!(
                f,
                "{}-{}",
                OPERATION_ID_STRING_PREFIX,
                bs58::encode(self.0.as_bytes()).into_string()
            )
        } else {
            write!(f, "{}", bs58::encode(self.0.as_bytes()).into_string())
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

// note: would be probably unused after the merge of
//       prefix
impl PreHashed for OperationId {}
impl Id for OperationId {
    fn new(hash: Hash) -> Self {
        OperationId(hash)
    }

    fn hash(&self) -> Hash {
        self.0
    }
}

impl PreHashed for OperationPrefixId {}

impl From<&[u8; OPERATION_ID_PREFIX_SIZE_BYTES]> for OperationPrefixId {
    /// get prefix of the operation id of size [OPERATION_ID_PREFIX_SIZE_BIT]
    fn from(bytes: &[u8; OPERATION_ID_PREFIX_SIZE_BYTES]) -> Self {
        Self(*bytes)
    }
}

impl From<&OperationPrefixId> for Vec<u8> {
    fn from(prefix: &OperationPrefixId) -> Self {
        prefix.0.to_vec()
    }
}

impl OperationId {
    /// op id to bytes
    pub fn to_bytes(&self) -> &[u8; OPERATION_ID_SIZE_BYTES] {
        self.0.to_bytes()
    }

    /// op id into bytes
    pub fn into_bytes(self) -> [u8; OPERATION_ID_SIZE_BYTES] {
        self.0.into_bytes()
    }

    /// op id from bytes
    pub fn from_bytes(data: &[u8; OPERATION_ID_SIZE_BYTES]) -> OperationId {
        OperationId(Hash::from_bytes(data))
    }

    /// op id from `bs58` check
    pub fn from_bs58_check(data: &str) -> Result<OperationId, ModelsError> {
        Ok(OperationId(
            Hash::from_bs58_check(data).map_err(|_| ModelsError::HashError)?,
        ))
    }

    /// convert the [OperationId] into a [OperationPrefixId]
    pub fn into_prefix(self) -> OperationPrefixId {
        OperationPrefixId(
            self.0.into_bytes()[..OPERATION_ID_PREFIX_SIZE_BYTES]
                .try_into()
                .expect("failed to truncate prefix from OperationId"),
        )
    }

    /// get a prefix from the [OperationId] by copying it
    pub fn prefix(&self) -> OperationPrefixId {
        OperationPrefixId(
            self.0.to_bytes()[..OPERATION_ID_PREFIX_SIZE_BYTES]
                .try_into()
                .expect("failed to truncate prefix from OperationId"),
        )
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
    /// the fee they have decided for this operation
    pub fee: Amount,
    /// after `expire_period` slot the operation won't be included in a block
    pub expire_period: u64,
    /// the type specific operation part
    pub op: OperationType,
}

impl std::fmt::Display for Operation {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Fee: {}", self.fee)?;
        writeln!(f, "Expire period: {}", self.expire_period)?;
        writeln!(f, "Operation type: {}", self.op)?;
        Ok(())
    }
}

/// signed operation
pub type WrappedOperation = Wrapped<Operation, OperationId>;

impl WrappedContent for Operation {}

/// Serializer for `Operation`
pub struct OperationSerializer {
    u64_serializer: U64VarIntSerializer,
    amount_serializer: AmountSerializer,
    op_type_serializer: OperationTypeSerializer,
}

impl OperationSerializer {
    /// Creates a new `OperationSerializer`
    pub fn new() -> Self {
        Self {
            u64_serializer: U64VarIntSerializer::new(),
            amount_serializer: AmountSerializer::new(),
            op_type_serializer: OperationTypeSerializer::new(),
        }
    }
}

impl Default for OperationSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<Operation> for OperationSerializer {
    fn serialize(&self, value: &Operation, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.amount_serializer.serialize(&value.fee, buffer)?;
        self.u64_serializer
            .serialize(&value.expire_period, buffer)?;
        self.op_type_serializer.serialize(&value.op, buffer)?;
        Ok(())
    }
}

/// Serializer for `Operation`
pub struct OperationDeserializer {
    u64_deserializer: U64VarIntDeserializer,
    amount_deserializer: AmountDeserializer,
    op_type_deserializer: OperationTypeDeserializer,
}

impl OperationDeserializer {
    /// Creates a `OperationDeserializer`
    pub const fn new() -> Self {
        Self {
            u64_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
            amount_deserializer: AmountDeserializer::new(Included(0), Included(u64::MAX)),
            op_type_deserializer: OperationTypeDeserializer::new(),
        }
    }
}

impl Default for OperationDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<Operation> for OperationDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Operation, E> {
        context(
            "Failed Operation deserialization",
            tuple((
                context("Failed fee deserialization", |input| {
                    self.amount_deserializer.deserialize(input)
                }),
                context("Failed expire_period deserialization", |input| {
                    self.u64_deserializer.deserialize(input)
                }),
                context("Failed op deserialization", |input| {
                    let (rest, op) = self.op_type_deserializer.deserialize(input)?;
                    Ok((rest, op))
                }),
            )),
        )
        .map(|(fee, expire_period, op)| Operation {
            fee,
            expire_period,
            op,
        })
        .parse(buffer)
    }
}

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

/// Serializer for `OperationType`
pub struct OperationTypeSerializer {
    u32_serializer: U32VarIntSerializer,
    u64_serializer: U64VarIntSerializer,
    vec_u8_serializer: VecU8Serializer,
    amount_serializer: AmountSerializer,
    function_name_serializer: StringSerializer<U16VarIntSerializer, u16>,
    parameter_serializer: StringSerializer<U16VarIntSerializer, u16>,
}

impl OperationTypeSerializer {
    /// Creates a new `OperationTypeSerializer`
    pub fn new() -> Self {
        Self {
            u32_serializer: U32VarIntSerializer::new(),
            u64_serializer: U64VarIntSerializer::new(),
            vec_u8_serializer: VecU8Serializer::new(),
            amount_serializer: AmountSerializer::new(),
            function_name_serializer: StringSerializer::new(U16VarIntSerializer::new()),
            parameter_serializer: StringSerializer::new(U16VarIntSerializer::new()),
        }
    }
}

impl Default for OperationTypeSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<OperationType> for OperationTypeSerializer {
    fn serialize(&self, value: &OperationType, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        match value {
            OperationType::Transaction {
                recipient_address,
                amount,
            } => {
                self.u32_serializer
                    .serialize(&u32::from(OperationTypeId::Transaction), buffer)?;
                buffer.extend(recipient_address.to_bytes());
                self.amount_serializer.serialize(amount, buffer)?;
            }
            OperationType::RollBuy { roll_count } => {
                self.u32_serializer
                    .serialize(&u32::from(OperationTypeId::RollBuy), buffer)?;
                self.u64_serializer.serialize(roll_count, buffer)?;
            }
            OperationType::RollSell { roll_count } => {
                self.u32_serializer
                    .serialize(&u32::from(OperationTypeId::RollSell), buffer)?;
                self.u64_serializer.serialize(roll_count, buffer)?;
            }
            OperationType::ExecuteSC {
                data,
                max_gas,
                coins,
                gas_price,
            } => {
                self.u32_serializer
                    .serialize(&u32::from(OperationTypeId::ExecuteSC), buffer)?;
                self.u64_serializer.serialize(max_gas, buffer)?;
                self.amount_serializer.serialize(coins, buffer)?;
                self.amount_serializer.serialize(gas_price, buffer)?;
                self.vec_u8_serializer.serialize(data, buffer)?;
            }
            OperationType::CallSC {
                target_addr,
                target_func,
                param,
                max_gas,
                sequential_coins,
                parallel_coins,
                gas_price,
            } => {
                self.u32_serializer
                    .serialize(&u32::from(OperationTypeId::CallSC), buffer)?;
                self.u64_serializer.serialize(max_gas, buffer)?;
                self.amount_serializer.serialize(parallel_coins, buffer)?;
                self.amount_serializer.serialize(sequential_coins, buffer)?;
                self.amount_serializer.serialize(gas_price, buffer)?;
                buffer.extend(target_addr.to_bytes());
                self.function_name_serializer
                    .serialize(target_func, buffer)?;
                self.parameter_serializer.serialize(param, buffer)?;
            }
        }
        Ok(())
    }
}

/// Serializer for `OperationType`
pub struct OperationTypeDeserializer {
    u32_deserializer: U32VarIntDeserializer,
    u64_deserializer: U64VarIntDeserializer,
    address_deserializer: AddressDeserializer,
    vec_u8_deserializer: VecU8Deserializer,
    amount_deserializer: AmountDeserializer,
    function_name_deserializer: StringDeserializer<U16VarIntDeserializer, u16>,
    parameter_deserializer: StringDeserializer<U32VarIntDeserializer, u32>,
}

impl OperationTypeDeserializer {
    /// Creates a new `OperationTypeDeserializer`
    pub const fn new() -> Self {
        Self {
            u32_deserializer: U32VarIntDeserializer::new(Included(0), Included(u32::MAX)),
            u64_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
            address_deserializer: AddressDeserializer::new(),
            vec_u8_deserializer: VecU8Deserializer::new(Included(0), Included(u64::MAX)),
            amount_deserializer: AmountDeserializer::new(Included(0), Included(u64::MAX)),
            function_name_deserializer: StringDeserializer::new(U16VarIntDeserializer::new(
                Included(0),
                Included(u16::MAX),
            )),
            parameter_deserializer: StringDeserializer::new(U32VarIntDeserializer::new(
                Included(0),
                Included(u32::MAX),
            )),
        }
    }
}

impl Default for OperationTypeDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<OperationType> for OperationTypeDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], OperationType, E> {
        context("Failed OperationType deserialization", |buffer| {
            let (input, id) = self.u32_deserializer.deserialize(buffer)?;
            let id = OperationTypeId::try_from(id).map_err(|_| {
                nom::Err::Error(ParseError::from_error_kind(
                    buffer,
                    nom::error::ErrorKind::Eof,
                ))
            })?;
            match id {
                OperationTypeId::Transaction => context(
                    "Failed Transaction deserialization",
                    tuple((
                        context("Failed recipient_address deserialization", |input| {
                            self.address_deserializer.deserialize(input)
                        }),
                        context("Failed amount deserialization", |input| {
                            self.amount_deserializer.deserialize(input)
                        }),
                    )),
                )
                .map(|(recipient_address, amount)| OperationType::Transaction {
                    recipient_address,
                    amount,
                })
                .parse(input),
                OperationTypeId::RollBuy => context("Failed RollBuy deserialization", |input| {
                    self.u64_deserializer.deserialize(input)
                })
                .map(|roll_count| OperationType::RollBuy { roll_count })
                .parse(input),
                OperationTypeId::RollSell => context("Failed RollSell deserialization", |input| {
                    self.u64_deserializer.deserialize(input)
                })
                .map(|roll_count| OperationType::RollSell { roll_count })
                .parse(input),
                OperationTypeId::ExecuteSC => context(
                    "Failed ExecuteSC deserialization",
                    tuple((
                        context("Failed max_gas deserialization", |input| {
                            self.u64_deserializer.deserialize(input)
                        }),
                        context("Failed coins deserialization", |input| {
                            self.amount_deserializer.deserialize(input)
                        }),
                        context("Failed gas_price deserialization", |input| {
                            self.amount_deserializer.deserialize(input)
                        }),
                        context("Failed data deserialization", |input| {
                            self.vec_u8_deserializer.deserialize(input)
                        }),
                    )),
                )
                .map(
                    |(max_gas, coins, gas_price, data)| OperationType::ExecuteSC {
                        data,
                        max_gas,
                        coins,
                        gas_price,
                    },
                )
                .parse(input),
                OperationTypeId::CallSC => context(
                    "Failed CallSC deserialization",
                    tuple((
                        context("Failed max_gas deserialization", |input| {
                            self.u64_deserializer.deserialize(input)
                        }),
                        context("Failed parallel_coins deserialization", |input| {
                            self.amount_deserializer.deserialize(input)
                        }),
                        context("Failed sequential_coins deserialization", |input| {
                            self.amount_deserializer.deserialize(input)
                        }),
                        context("Failed gas_price deserialization", |input| {
                            self.amount_deserializer.deserialize(input)
                        }),
                        context("Failed target_addr deserialization", |input| {
                            self.address_deserializer.deserialize(input)
                        }),
                        context("Failed target_func deserialization", |input| {
                            self.function_name_deserializer.deserialize(input)
                        }),
                        context("Failed param deserialization", |input| {
                            self.parameter_deserializer.deserialize(input)
                        }),
                    )),
                )
                .map(
                    |(
                        max_gas,
                        parallel_coins,
                        sequential_coins,
                        gas_price,
                        target_addr,
                        target_func,
                        param,
                    )| OperationType::CallSC {
                        target_addr,
                        target_func,
                        param,
                        max_gas,
                        sequential_coins,
                        parallel_coins,
                        gas_price,
                    },
                )
                .parse(input),
            }
        })
        .parse(buffer)
    }
}

impl WrappedOperation {
    /// Verifies the signature and integrity of the operation and computes operation ID
    pub fn verify_integrity(&self) -> Result<OperationId, ModelsError> {
        self.verify_signature(OperationSerializer::new(), &self.creator_public_key)?;
        Ok(self.id)
    }
}

impl WrappedOperation {
    /// get the range of periods during which an operation is valid
    pub fn get_validity_range(&self, operation_validity_period: u64) -> RangeInclusive<u64> {
        let start = self
            .content
            .expire_period
            .saturating_sub(operation_validity_period);
        start..=self.content.expire_period
    }

    /// Get the amount of gas used by the operation
    pub fn get_gas_usage(&self) -> u64 {
        match &self.content.op {
            OperationType::ExecuteSC { max_gas, .. } => *max_gas,
            OperationType::CallSC { max_gas, .. } => *max_gas,
            OperationType::RollBuy { .. } => 0,
            OperationType::RollSell { .. } => 0,
            OperationType::Transaction { .. } => 0,
        }
    }

    /// Get the amount of coins used by the operation to pay for gas
    pub fn get_gas_coins(&self) -> Amount {
        match &self.content.op {
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
    pub fn get_ledger_involved_addresses(&self) -> Set<Address> {
        let mut res = Set::<Address>::default();
        let emitter_address = Address::from_public_key(&self.creator_public_key);
        res.insert(emitter_address);
        match &self.content.op {
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
        res
    }

    /// get the addresses that are involved in this operation from a rolls point of view
    pub fn get_roll_involved_addresses(&self) -> Result<Set<Address>, ModelsError> {
        let mut res = Set::<Address>::default();
        match self.content.op {
            OperationType::Transaction { .. } => {}
            OperationType::RollBuy { .. } => {
                res.insert(Address::from_public_key(&self.creator_public_key));
            }
            OperationType::RollSell { .. } => {
                res.insert(Address::from_public_key(&self.creator_public_key));
            }
            OperationType::ExecuteSC { .. } => {}
            OperationType::CallSC { .. } => {}
        }
        Ok(res)
    }
}

/// Set of operation ids
pub type OperationIds = Set<OperationId>;
/// Set of operation id's prefix
pub type OperationPrefixIds = Set<OperationPrefixId>;

/// Serializer for `OperationIds`
pub struct OperationIdsSerializer {
    u32_serializer: U32VarIntSerializer,
}

impl OperationIdsSerializer {
    /// Creates a new `OperationIdsSerializer`
    pub fn new() -> Self {
        Self {
            u32_serializer: U32VarIntSerializer::new(),
        }
    }
}

impl Default for OperationIdsSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<OperationIds> for OperationIdsSerializer {
    fn serialize(&self, value: &OperationIds, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        let list_len: u32 = value.len().try_into().map_err(|_| {
            SerializeError::NumberTooBig("could not encode OperationIds list length as u32".into())
        })?;
        self.u32_serializer.serialize(&list_len, buffer)?;
        for hash in value {
            buffer.extend(hash.into_bytes());
        }
        Ok(())
    }
}

/// Deserializer for `OperationIds`
pub struct OperationIdsDeserializer {
    u32_deserializer: U32VarIntDeserializer,
    hash_deserializer: HashDeserializer,
}

impl OperationIdsDeserializer {
    /// Creates a new `OperationIdsDeserializer`
    pub fn new() -> Self {
        Self {
            u32_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(MAX_OPERATIONS_PER_MESSAGE),
            ),
            hash_deserializer: HashDeserializer::new(),
        }
    }
}

impl Default for OperationIdsDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<OperationIds> for OperationIdsDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], OperationIds, E> {
        context(
            "Failed OperationIds deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.u32_deserializer.deserialize(input)
                }),
                context("Failed OperationId deserialization", |input| {
                    self.hash_deserializer.deserialize(input)
                }),
            ),
        )
        .map(|hashes| hashes.into_iter().map(OperationId).collect())
        .parse(buffer)
    }
}

/// Deserializer for [OperationPrefixId]
#[derive(Default)]
pub struct OperationPrefixIdDeserializer;

impl OperationPrefixIdDeserializer {
    /// Creates a deserializer for [OperationPrefixId]
    pub const fn new() -> Self {
        Self
    }
}

impl Deserializer<OperationPrefixId> for OperationPrefixIdDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], OperationPrefixId, E> {
        context(
            "Failed operation prefix id deserialization",
            |input: &'a [u8]| {
                if buffer.len() < OPERATION_ID_PREFIX_SIZE_BYTES {
                    return Err(nom::Err::Error(ParseError::from_error_kind(
                        input,
                        nom::error::ErrorKind::LengthValue,
                    )));
                }
                Ok((
                    &buffer[OPERATION_ID_PREFIX_SIZE_BYTES..],
                    OperationPrefixId::from(
                        &buffer[..OPERATION_ID_PREFIX_SIZE_BYTES]
                            .try_into()
                            .map_err(|_| {
                                nom::Err::Error(ParseError::from_error_kind(
                                    input,
                                    nom::error::ErrorKind::Fail,
                                ))
                            })?,
                    ),
                ))
            },
        )(buffer)
    }
}

/// Deserializer for `OperationPrefixIds`
pub struct OperationPrefixIdsDeserializer {
    u32_deserializer: U32VarIntDeserializer,
    pref_deserializer: OperationPrefixIdDeserializer,
}

impl OperationPrefixIdsDeserializer {
    /// Creates a new `OperationIdsDeserializer`
    pub const fn new() -> Self {
        Self {
            u32_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(MAX_OPERATIONS_PER_MESSAGE),
            ),
            pref_deserializer: OperationPrefixIdDeserializer::new(),
        }
    }
}

impl Default for OperationPrefixIdsDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<OperationPrefixIds> for OperationPrefixIdsDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], OperationPrefixIds, E> {
        context(
            "Failed OperationPrefixIds deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.u32_deserializer.deserialize(input)
                }),
                context("Failed OperationPrefixId deserialization", |input| {
                    self.pref_deserializer.deserialize(input)
                }),
            ),
        )
        .map(|hashes| hashes.into_iter().collect())
        .parse(buffer)
    }
}

/// Serializer for `OperationPrefixIds`
pub struct OperationPrefixIdsSerializer {
    u32_serializer: U32VarIntSerializer,
}

impl OperationPrefixIdsSerializer {
    /// Creates a new `OperationIdsSerializer`
    pub const fn new() -> Self {
        Self {
            u32_serializer: U32VarIntSerializer::new(),
        }
    }
}

impl Default for OperationPrefixIdsSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<OperationPrefixIds> for OperationPrefixIdsSerializer {
    fn serialize(
        &self,
        value: &OperationPrefixIds,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        let list_len: u32 = value.len().try_into().map_err(|_| {
            SerializeError::NumberTooBig("could not encode OperationIds list length as u32".into())
        })?;
        self.u32_serializer.serialize(&list_len, buffer)?;
        for prefix in value {
            buffer.extend(Vec::<u8>::from(prefix));
        }
        Ok(())
    }
}

/// Set of self containing signed operations.
pub type Operations = Vec<WrappedOperation>;

/// Serializer for `Operations`
pub struct OperationsSerializer {
    u32_serializer: U32VarIntSerializer,
    signed_op_serializer: WrappedSerializer,
}

impl OperationsSerializer {
    /// Creates a new `OperationsSerializer`
    pub const fn new() -> Self {
        Self {
            u32_serializer: U32VarIntSerializer::new(),
            signed_op_serializer: WrappedSerializer::new(),
        }
    }
}

impl Default for OperationsSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<Operations> for OperationsSerializer {
    fn serialize(&self, value: &Operations, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        let list_len: u32 = value.len().try_into().map_err(|_| {
            SerializeError::NumberTooBig("could not encode Operations list length as u32".into())
        })?;
        self.u32_serializer.serialize(&list_len, buffer)?;
        for op in value {
            self.signed_op_serializer.serialize(op, buffer)?;
        }
        Ok(())
    }
}

/// Deserializer for `Operations`
pub struct OperationsDeserializer {
    u32_deserializer: U32VarIntDeserializer,
    signed_op_deserializer: WrappedDeserializer<Operation, OperationDeserializer>,
}

impl OperationsDeserializer {
    /// Creates a new `OperationsDeserializer`
    pub const fn new() -> Self {
        Self {
            u32_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(MAX_OPERATIONS_PER_MESSAGE),
            ),
            signed_op_deserializer: WrappedDeserializer::new(OperationDeserializer::new()),
        }
    }
}

impl Default for OperationsDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<Operations> for OperationsDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Operations, E> {
        context(
            "Failed Operations deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.u32_deserializer.deserialize(input)
                }),
                context("Failed operation deserialization", |input| {
                    self.signed_op_deserializer.deserialize(input)
                }),
            ),
        )
        .parse(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use massa_serialization::DeserializeError;
    use massa_signature::KeyPair;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_transaction() {
        let sender_keypair = KeyPair::generate();
        let recv_keypair = KeyPair::generate();

        let op = OperationType::Transaction {
            recipient_address: Address::from_public_key(&recv_keypair.get_public_key()),
            amount: Amount::default(),
        };
        let mut ser_type = Vec::new();
        OperationTypeSerializer::new()
            .serialize(&op, &mut ser_type)
            .unwrap();
        let (_, res_type) = OperationTypeDeserializer::new()
            .deserialize::<DeserializeError>(&ser_type)
            .unwrap();
        assert_eq!(format!("{}", res_type), format!("{}", op));

        let content = Operation {
            fee: Amount::from_str("20").unwrap(),
            op,
            expire_period: 50,
        };

        let mut ser_content = Vec::new();
        OperationSerializer::new()
            .serialize(&content, &mut ser_content)
            .unwrap();
        let (_, res_content) = OperationDeserializer::new()
            .deserialize::<DeserializeError>(&ser_content)
            .unwrap();
        assert_eq!(format!("{}", res_content), format!("{}", content));
        let op_serializer = OperationSerializer::new();

        let op = Operation::new_wrapped(content, op_serializer, &sender_keypair).unwrap();

        let mut ser_op = Vec::new();
        WrappedSerializer::new()
            .serialize(&op, &mut ser_op)
            .unwrap();
        let (_, res_op): (&[u8], WrappedOperation) =
            WrappedDeserializer::new(OperationDeserializer::new())
                .deserialize::<DeserializeError>(&ser_op)
                .unwrap();
        assert_eq!(format!("{}", res_op), format!("{}", op));

        assert_eq!(op.get_validity_range(10), 40..=50);
    }

    #[test]
    #[serial]
    fn test_executesc() {
        let sender_keypair = KeyPair::generate();

        let op = OperationType::ExecuteSC {
            max_gas: 123,
            coins: Amount::from_str("456.789").unwrap(),
            gas_price: Amount::from_str("772.122").unwrap(),
            data: vec![23u8, 123u8, 44u8],
        };
        let mut ser_type = Vec::new();
        OperationTypeSerializer::new()
            .serialize(&op, &mut ser_type)
            .unwrap();
        let (_, res_type) = OperationTypeDeserializer::new()
            .deserialize::<DeserializeError>(&ser_type)
            .unwrap();
        assert_eq!(format!("{}", res_type), format!("{}", op));

        let content = Operation {
            fee: Amount::from_str("20").unwrap(),
            op,
            expire_period: 50,
        };

        let mut ser_content = Vec::new();
        OperationSerializer::new()
            .serialize(&content, &mut ser_content)
            .unwrap();
        let (_, res_content) = OperationDeserializer::new()
            .deserialize::<DeserializeError>(&ser_content)
            .unwrap();
        assert_eq!(format!("{}", res_content), format!("{}", content));
        let op_serializer = OperationSerializer::new();

        let op = Operation::new_wrapped(content, op_serializer, &sender_keypair).unwrap();

        let mut ser_op = Vec::new();
        WrappedSerializer::new()
            .serialize(&op, &mut ser_op)
            .unwrap();
        let (_, res_op): (&[u8], WrappedOperation) =
            WrappedDeserializer::new(OperationDeserializer::new())
                .deserialize::<DeserializeError>(&ser_op)
                .unwrap();
        assert_eq!(format!("{}", res_op), format!("{}", op));

        assert_eq!(op.get_validity_range(10), 40..=50);
    }

    #[test]
    #[serial]
    fn test_callsc() {
        let sender_keypair = KeyPair::generate();

        let target_keypair = KeyPair::generate();
        let target_addr = Address::from_public_key(&target_keypair.get_public_key());

        let op = OperationType::CallSC {
            max_gas: 123,
            target_addr,
            parallel_coins: Amount::from_str("456.789").unwrap(),
            sequential_coins: Amount::from_str("123.111").unwrap(),
            gas_price: Amount::from_str("772.122").unwrap(),
            target_func: "target function".to_string(),
            param: "parameter".to_string(),
        };
        let mut ser_type = Vec::new();
        OperationTypeSerializer::new()
            .serialize(&op, &mut ser_type)
            .unwrap();
        let (_, res_type) = OperationTypeDeserializer::new()
            .deserialize::<DeserializeError>(&ser_type)
            .unwrap();
        assert_eq!(format!("{}", res_type), format!("{}", op));

        let content = Operation {
            fee: Amount::from_str("20").unwrap(),
            op,
            expire_period: 50,
        };

        let mut ser_content = Vec::new();
        OperationSerializer::new()
            .serialize(&content, &mut ser_content)
            .unwrap();
        let (_, res_content) = OperationDeserializer::new()
            .deserialize::<DeserializeError>(&ser_content)
            .unwrap();
        assert_eq!(format!("{}", res_content), format!("{}", content));
        let op_serializer = OperationSerializer::new();

        let op = Operation::new_wrapped(content, op_serializer, &sender_keypair).unwrap();

        let mut ser_op = Vec::new();
        WrappedSerializer::new()
            .serialize(&op, &mut ser_op)
            .unwrap();
        let (_, res_op): (&[u8], WrappedOperation) =
            WrappedDeserializer::new(OperationDeserializer::new())
                .deserialize::<DeserializeError>(&ser_op)
                .unwrap();
        assert_eq!(format!("{}", res_op), format!("{}", op));

        assert_eq!(op.get_validity_range(10), 40..=50);
    }
}
