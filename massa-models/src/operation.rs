// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::datastore::{Datastore, DatastoreDeserializer, DatastoreSerializer};
use crate::prehash::{PreHashSet, PreHashed};
use crate::wrapped::{Id, Wrapped, WrappedContent, WrappedDeserializer, WrappedSerializer};
use crate::{
    address::{Address, AddressDeserializer},
    amount::{Amount, AmountDeserializer, AmountSerializer},
    error::ModelsError,
    serialization::{StringDeserializer, StringSerializer, VecU8Deserializer, VecU8Serializer},
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

/// Size in bytes of the serialized operation ID
pub const OPERATION_ID_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;

/// Size in bytes of the serialized operation ID prefix
pub const OPERATION_ID_PREFIX_SIZE_BYTES: usize = 17;

/// operation id
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct OperationId(Hash);

/// Left part of the operation id hash stored in a vector of size [`OPERATION_ID_PREFIX_SIZE_BYTES`]
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct OperationPrefixId([u8; OPERATION_ID_PREFIX_SIZE_BYTES]);

impl std::fmt::Display for OperationId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0.to_bs58_check())
    }
}

impl std::fmt::Debug for OperationId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0.to_bs58_check())
    }
}

impl std::fmt::Display for OperationPrefixId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", bs58::encode(self.0.as_bytes()).into_string())
    }
}

impl std::fmt::Debug for OperationPrefixId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", bs58::encode(self.0.as_bytes()).into_string())
    }
}

impl FromStr for OperationId {
    type Err = ModelsError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(OperationId(Hash::from_str(s)?))
    }
}

// note: would be probably unused after the merge of
//       prefix
impl PreHashed for OperationId {}
impl Id for OperationId {
    fn new(hash: Hash) -> Self {
        OperationId(hash)
    }

    fn get_hash(&self) -> &Hash {
        &self.0
    }
}

impl PreHashed for OperationPrefixId {}

impl From<&[u8; OPERATION_ID_PREFIX_SIZE_BYTES]> for OperationPrefixId {
    /// get prefix of the operation id of size `OPERATION_ID_PREFIX_SIZE_BIT`
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

    /// convert the [`OperationId`] into a [`OperationPrefixId`]
    pub fn into_prefix(self) -> OperationPrefixId {
        OperationPrefixId(
            self.0.into_bytes()[..OPERATION_ID_PREFIX_SIZE_BYTES]
                .try_into()
                .expect("failed to truncate prefix from OperationId"),
        )
    }

    /// get a prefix from the [`OperationId`] by copying it
    pub fn prefix(&self) -> OperationPrefixId {
        OperationPrefixId(
            self.0.to_bytes()[..OPERATION_ID_PREFIX_SIZE_BYTES]
                .try_into()
                .expect("failed to truncate prefix from OperationId"),
        )
    }
}

/// Serializer for `OperationId`
#[derive(Default, Clone)]
pub struct OperationIdSerializer;

impl OperationIdSerializer {
    /// Creates a new serializer for `OperationId`
    pub fn new() -> Self {
        Self
    }
}

impl Serializer<OperationId> for OperationIdSerializer {
    fn serialize(&self, value: &OperationId, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        buffer.extend(value.to_bytes());
        Ok(())
    }
}

/// Deserializer for `OperationId`
#[derive(Default, Clone)]
pub struct OperationIdDeserializer {
    hash_deserializer: HashDeserializer,
}

impl OperationIdDeserializer {
    /// Creates a new deserializer for `OperationId`
    pub fn new() -> Self {
        Self {
            hash_deserializer: HashDeserializer::new(),
        }
    }
}

impl Deserializer<OperationId> for OperationIdDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], OperationId, E> {
        context("Failed OperationId deserialization", |input| {
            let (rest, hash) = self.hash_deserializer.deserialize(input)?;
            Ok((rest, OperationId(hash)))
        })(buffer)
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
    /// ## Example:
    /// ```rust
    /// use massa_models::{amount::Amount, address::Address, operation::{OperationType, OperationSerializer, Operation}};
    /// use massa_signature::KeyPair;
    /// use massa_serialization::Serializer;
    /// use std::str::FromStr;
    ///
    /// let keypair = KeyPair::generate();
    /// let op = OperationType::Transaction {
    ///    recipient_address: Address::from_public_key(&keypair.get_public_key()),
    ///    amount: Amount::from_str("300").unwrap(),
    /// };
    /// let operation = Operation {
    ///   fee: Amount::from_str("20").unwrap(),
    ///   op,
    ///   expire_period: 50,
    /// };
    /// let mut buffer = Vec::new();
    /// OperationSerializer::new().serialize(&operation, &mut buffer).unwrap();
    /// ```
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
    expire_period_deserializer: U64VarIntDeserializer,
    amount_deserializer: AmountDeserializer,
    op_type_deserializer: OperationTypeDeserializer,
}

impl OperationDeserializer {
    /// Creates a `OperationDeserializer`
    pub fn new(
        max_datastore_value_length: u64,
        max_function_name_length: u16,
        max_parameters_size: u32,
        max_op_datastore_entry_count: u64,
        max_op_datastore_key_length: u8,
        max_op_datastore_value_length: u64,
    ) -> Self {
        Self {
            expire_period_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
            amount_deserializer: AmountDeserializer::new(
                Included(Amount::MIN),
                Included(Amount::MAX),
            ),
            op_type_deserializer: OperationTypeDeserializer::new(
                max_datastore_value_length,
                max_function_name_length,
                max_parameters_size,
                max_op_datastore_entry_count,
                max_op_datastore_key_length,
                max_op_datastore_value_length,
            ),
        }
    }
}

impl Deserializer<Operation> for OperationDeserializer {
    /// ## Example:
    /// ```rust
    /// use massa_models::{amount::Amount, address::Address, operation::{OperationType, OperationSerializer, Operation, OperationDeserializer}};
    /// use massa_signature::KeyPair;
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use std::str::FromStr;
    ///
    /// let keypair = KeyPair::generate();
    /// let op = OperationType::Transaction {
    ///    recipient_address: Address::from_public_key(&keypair.get_public_key()),
    ///    amount: Amount::from_str("300").unwrap(),
    /// };
    /// let operation = Operation {
    ///   fee: Amount::from_str("20").unwrap(),
    ///   op,
    ///   expire_period: 50,
    /// };
    /// let mut buffer = Vec::new();
    /// OperationSerializer::new().serialize(&operation, &mut buffer).unwrap();
    /// let (rest, deserialized_operation) = OperationDeserializer::new(10000, 10000, 10000, 100, 255, 10_000).deserialize::<DeserializeError>(&buffer).unwrap();
    /// assert_eq!(rest.len(), 0);
    /// assert_eq!(deserialized_operation.fee, operation.fee);
    /// assert_eq!(deserialized_operation.expire_period, operation.expire_period);
    /// match deserialized_operation.op {
    ///   OperationType::Transaction {
    ///     recipient_address,
    ///     amount,
    ///   } => {
    ///     assert_eq!(recipient_address, Address::from_public_key(&keypair.get_public_key()));
    ///     assert_eq!(amount, Amount::from_str("300").unwrap());
    ///   }
    ///   _ => panic!("wrong operation type"),
    /// };
    /// ```
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
                    self.expire_period_deserializer.deserialize(input)
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
        /// The price per unit of gas that the caller is willing to pay for the execution.
        gas_price: Amount,
        /// A key-value store associating a hash to arbitrary bytes
        datastore: Datastore,
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
        /// Extra coins that are spent from the caller's balance and transferred to the target
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
                writeln!(f, "\t- Roll count:{}", roll_count)?;
            }
            OperationType::RollSell { roll_count } => {
                writeln!(f, "Sell rolls:")?;
                writeln!(f, "\t- Roll count:{}", roll_count)?;
            }
            OperationType::ExecuteSC {
                max_gas,
                gas_price,
                ..
                // data & datastore, // these fields are ignored because bytes eh
            } => {
                writeln!(f, "ExecuteSC: ")?;
                writeln!(f, "\t- max_gas:{}", max_gas)?;
                writeln!(f, "\t- gas_price:{}", gas_price)?;
            },
            OperationType::CallSC {
                max_gas,
                coins,
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
                writeln!(f, "\t- coins:{}", coins)?;
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
    parameter_serializer: StringSerializer<U32VarIntSerializer, u32>,
    datastore_serializer: DatastoreSerializer,
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
            parameter_serializer: StringSerializer::new(U32VarIntSerializer::new()),
            datastore_serializer: DatastoreSerializer::new(),
        }
    }
}

impl Default for OperationTypeSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<OperationType> for OperationTypeSerializer {
    /// ## Example:
    /// ```rust
    /// use std::collections::BTreeMap;
    /// use massa_models::{operation::{OperationTypeSerializer, OperationTypeDeserializer,OperationType}, address::Address, amount::Amount};
    /// use massa_signature::KeyPair;
    /// use massa_serialization::{Deserializer, Serializer, DeserializeError};
    /// use std::str::FromStr;
    ///
    /// let keypair = KeyPair::generate();
    /// let op = OperationType::ExecuteSC {
    ///    data: vec![0x01, 0x02, 0x03],
    ///    max_gas: 100,
    ///    gas_price: Amount::from_str("1").unwrap(),
    ///    datastore: BTreeMap::default(),
    /// };
    /// let mut buffer = Vec::new();
    /// OperationTypeSerializer::new().serialize(&op, &mut buffer).unwrap();
    /// ```
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
                gas_price,
                datastore,
            } => {
                self.u32_serializer
                    .serialize(&u32::from(OperationTypeId::ExecuteSC), buffer)?;
                self.u64_serializer.serialize(max_gas, buffer)?;
                self.amount_serializer.serialize(gas_price, buffer)?;
                self.vec_u8_serializer.serialize(data, buffer)?;
                self.datastore_serializer.serialize(datastore, buffer)?;
            }
            OperationType::CallSC {
                target_addr,
                target_func,
                param,
                max_gas,
                coins,
                gas_price,
            } => {
                self.u32_serializer
                    .serialize(&u32::from(OperationTypeId::CallSC), buffer)?;
                self.u64_serializer.serialize(max_gas, buffer)?;
                self.amount_serializer.serialize(coins, buffer)?;
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
    id_deserializer: U32VarIntDeserializer,
    rolls_number_deserializer: U64VarIntDeserializer,
    max_gas_deserializer: U64VarIntDeserializer,
    address_deserializer: AddressDeserializer,
    data_deserializer: VecU8Deserializer,
    amount_deserializer: AmountDeserializer,
    function_name_deserializer: StringDeserializer<U16VarIntDeserializer, u16>,
    parameter_deserializer: StringDeserializer<U32VarIntDeserializer, u32>,
    datastore_deserializer: DatastoreDeserializer,
}

impl OperationTypeDeserializer {
    /// Creates a new `OperationTypeDeserializer`
    pub fn new(
        max_datastore_value_length: u64,
        max_function_name_length: u16,
        max_parameters_size: u32,
        max_op_datastore_entry_count: u64,
        max_op_datastore_key_length: u8,
        max_op_datastore_value_length: u64,
    ) -> Self {
        Self {
            id_deserializer: U32VarIntDeserializer::new(Included(0), Included(u32::MAX)),
            rolls_number_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
            max_gas_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
            address_deserializer: AddressDeserializer::new(),
            data_deserializer: VecU8Deserializer::new(
                Included(0),
                Included(max_datastore_value_length),
            ),
            amount_deserializer: AmountDeserializer::new(
                Included(Amount::MIN),
                Included(Amount::MAX),
            ),
            function_name_deserializer: StringDeserializer::new(U16VarIntDeserializer::new(
                Included(0),
                Included(max_function_name_length),
            )),
            parameter_deserializer: StringDeserializer::new(U32VarIntDeserializer::new(
                Included(0),
                Included(max_parameters_size),
            )),
            datastore_deserializer: DatastoreDeserializer::new(
                max_op_datastore_entry_count,
                max_op_datastore_key_length,
                max_op_datastore_value_length,
            ),
        }
    }
}

impl Deserializer<OperationType> for OperationTypeDeserializer {
    /// ## Example:
    /// ```rust
    /// use std::collections::BTreeMap;
    /// use massa_models::{operation::{OperationTypeSerializer, OperationTypeDeserializer, OperationType}, address::Address, amount::Amount};
    /// use massa_signature::KeyPair;
    /// use massa_serialization::{Deserializer, Serializer, DeserializeError};
    /// use std::str::FromStr;
    ///
    /// let keypair = KeyPair::generate();
    /// let op = OperationType::ExecuteSC {
    ///    data: vec![0x01, 0x02, 0x03],
    ///    max_gas: 100,
    ///    gas_price: Amount::from_str("1").unwrap(),
    ///    datastore: BTreeMap::from([(vec![1, 2], vec![254, 255])])
    /// };
    /// let mut buffer = Vec::new();
    /// OperationTypeSerializer::new().serialize(&op, &mut buffer).unwrap();
    /// let (rest, op_deserialized) = OperationTypeDeserializer::new(10000, 10000, 10000, 10, 255, 10_000).deserialize::<DeserializeError>(&buffer).unwrap();
    /// assert_eq!(rest.len(), 0);
    /// match op_deserialized {
    ///    OperationType::ExecuteSC {
    ///      data,
    ///      max_gas,
    ///      gas_price,
    ///      datastore
    ///   } => {
    ///     assert_eq!(data, vec![0x01, 0x02, 0x03]);
    ///     assert_eq!(max_gas, 100);
    ///     assert_eq!(gas_price, Amount::from_str("1").unwrap());
    ///     assert_eq!(datastore, BTreeMap::from([(vec![1, 2], vec![254, 255])]))
    ///   }
    ///   _ => panic!("Unexpected operation type"),
    /// };
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], OperationType, E> {
        context("Failed OperationType deserialization", |buffer| {
            let (input, id) = self.id_deserializer.deserialize(buffer)?;
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
                    self.rolls_number_deserializer.deserialize(input)
                })
                .map(|roll_count| OperationType::RollBuy { roll_count })
                .parse(input),
                OperationTypeId::RollSell => context("Failed RollSell deserialization", |input| {
                    self.rolls_number_deserializer.deserialize(input)
                })
                .map(|roll_count| OperationType::RollSell { roll_count })
                .parse(input),
                OperationTypeId::ExecuteSC => context(
                    "Failed ExecuteSC deserialization",
                    tuple((
                        context("Failed max_gas deserialization", |input| {
                            self.max_gas_deserializer.deserialize(input)
                        }),
                        context("Failed gas_price deserialization", |input| {
                            self.amount_deserializer.deserialize(input)
                        }),
                        context("Failed data deserialization", |input| {
                            self.data_deserializer.deserialize(input)
                        }),
                        context("Failed datastore deserialization", |input| {
                            self.datastore_deserializer.deserialize(input)
                        }),
                    )),
                )
                .map(
                    |(max_gas, gas_price, data, datastore)| OperationType::ExecuteSC {
                        data,
                        max_gas,
                        gas_price,
                        datastore,
                    },
                )
                .parse(input),
                OperationTypeId::CallSC => context(
                    "Failed CallSC deserialization",
                    tuple((
                        context("Failed max_gas deserialization", |input| {
                            self.max_gas_deserializer.deserialize(input)
                        }),
                        context("Failed coins deserialization", |input| {
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
                    |(max_gas, coins, gas_price, target_addr, target_func, param)| {
                        OperationType::CallSC {
                            target_addr,
                            target_func,
                            param,
                            max_gas,
                            coins,
                            gas_price,
                        }
                    },
                )
                .parse(input),
            }
        })
        .parse(buffer)
    }
}

impl WrappedOperation {
    /// get the range of periods during which an operation is valid
    /// Range: `(op.expire_period - cfg.operation_validity_period) -> op.expire_period` (included)
    pub fn get_validity_range(&self, operation_validity_period: u64) -> RangeInclusive<u64> {
        let start = self
            .content
            .expire_period
            .saturating_sub(operation_validity_period);
        start..=self.content.expire_period
    }

    /// Get the max amount of gas used by the operation (`max_gas`)
    pub fn get_gas_usage(&self) -> u64 {
        match &self.content.op {
            OperationType::ExecuteSC { max_gas, .. } => *max_gas,
            OperationType::CallSC { max_gas, .. } => *max_gas,
            OperationType::RollBuy { .. } => 0,
            OperationType::RollSell { .. } => 0,
            OperationType::Transaction { .. } => 0,
        }
    }

    /// Get the gas price set by the operation
    pub fn get_gas_price(&self) -> Amount {
        match &self.content.op {
            OperationType::ExecuteSC { gas_price, .. } => *gas_price,
            OperationType::CallSC { gas_price, .. } => *gas_price,
            OperationType::RollBuy { .. } => Amount::default(),
            OperationType::RollSell { .. } => Amount::default(),
            OperationType::Transaction { .. } => Amount::default(),
        }
    }

    /// Get the amount of coins used by the operation to pay for gas
    pub fn get_gas_coins(&self) -> Amount {
        self.get_gas_price()
            .saturating_mul_u64(self.get_gas_usage())
    }

    /// Get the total fee paid by the creator
    pub fn get_total_fee(&self) -> Amount {
        self.get_gas_coins().saturating_add(self.content.fee)
    }

    /// get the addresses that are involved in this operation from a ledger point of view
    pub fn get_ledger_involved_addresses(&self) -> PreHashSet<Address> {
        let mut res = PreHashSet::<Address>::default();
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

    /// Gets the maximal amount of coins that may be spent by this operation (incl. fee)
    pub fn get_max_spending(&self, roll_price: Amount) -> Amount {
        // compute the max amount of coins spent outside of the fees
        let max_non_fee_seq_spending = match &self.content.op {
            OperationType::Transaction { amount, .. } => *amount,
            OperationType::RollBuy { roll_count } => roll_price.saturating_mul_u64(*roll_count),
            OperationType::RollSell { .. } => Amount::zero(),
            OperationType::ExecuteSC {
                max_gas, gas_price, ..
            } => gas_price.saturating_mul_u64(*max_gas),
            OperationType::CallSC {
                max_gas,
                gas_price,
                coins,
                ..
            } => gas_price
                .saturating_mul_u64(*max_gas)
                .saturating_add(*coins),
        };

        // add all fees and return
        max_non_fee_seq_spending.saturating_add(self.get_total_fee())
    }

    /// get the addresses that are involved in this operation from a rolls point of view
    pub fn get_roll_involved_addresses(&self) -> Result<PreHashSet<Address>, ModelsError> {
        let mut res = PreHashSet::<Address>::default();
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

/// Set of operation id's prefix
pub type OperationPrefixIds = PreHashSet<OperationPrefixId>;

/// Serializer for `Vec<OperationId>`
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

impl Serializer<Vec<OperationId>> for OperationIdsSerializer {
    /// ## Example:
    /// ```
    /// use massa_models::operation::{OperationId, OperationIdsSerializer};
    /// use massa_serialization::Serializer;
    /// use std::str::FromStr;
    ///
    /// let mut operations_ids = Vec::new();
    /// operations_ids.push(OperationId::from_str("2AGSu2kBG9FZ649h18F82CYfsymkhVH2epMafMN2sPZNBQXTrz").unwrap());
    /// operations_ids.push(OperationId::from_str("2AGSu2kBG9FZ649h18F82CYfsymkhVH2epMafMN2sPZNBQXTrz").unwrap());
    /// let mut buffer = Vec::new();
    /// OperationIdsSerializer::new().serialize(&operations_ids, &mut buffer).unwrap();
    /// ```
    fn serialize(
        &self,
        value: &Vec<OperationId>,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        let list_len: u32 = value.len().try_into().map_err(|_| {
            SerializeError::NumberTooBig(
                "could not encode Vec<OperationId> list length as u32".into(),
            )
        })?;
        self.u32_serializer.serialize(&list_len, buffer)?;
        for hash in value {
            buffer.extend(hash.into_bytes());
        }
        Ok(())
    }
}

/// Deserializer for `Vec<OperationId>`
pub struct OperationIdsDeserializer {
    length_deserializer: U32VarIntDeserializer,
    hash_deserializer: HashDeserializer,
}

impl OperationIdsDeserializer {
    /// Creates a new `OperationIdsDeserializer`
    pub fn new(max_operations_per_message: u32) -> Self {
        Self {
            length_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(max_operations_per_message),
            ),
            hash_deserializer: HashDeserializer::new(),
        }
    }
}

impl Deserializer<Vec<OperationId>> for OperationIdsDeserializer {
    /// ## Example:
    /// ```
    /// use massa_models::operation::{OperationId, OperationIdsSerializer, OperationIdsDeserializer};
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use std::str::FromStr;
    ///
    /// let mut operations_ids = Vec::new();
    /// operations_ids.push(OperationId::from_str("2AGSu2kBG9FZ649h18F82CYfsymkhVH2epMafMN2sPZNBQXTrz").unwrap());
    /// operations_ids.push(OperationId::from_str("2AGSu2kBG9FZ649h18F82CYfsymkhVH2epMafMN2sPZNBQXTrz").unwrap());
    /// let mut buffer = Vec::new();
    /// OperationIdsSerializer::new().serialize(&operations_ids, &mut buffer).unwrap();
    /// let (rest, deserialized_operations_ids) = OperationIdsDeserializer::new(1000).deserialize::<DeserializeError>(&buffer).unwrap();
    /// assert_eq!(rest.len(), 0);
    /// assert_eq!(deserialized_operations_ids, operations_ids);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Vec<OperationId>, E> {
        context(
            "Failed Vec<OperationId> deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.length_deserializer.deserialize(input)
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

/// Deserializer for [`OperationPrefixId`]
#[derive(Default)]
pub struct OperationPrefixIdDeserializer;

impl OperationPrefixIdDeserializer {
    /// Creates a deserializer for [`OperationPrefixId`]
    pub const fn new() -> Self {
        Self
    }
}

impl Deserializer<OperationPrefixId> for OperationPrefixIdDeserializer {
    /// ## Example:
    /// ```rust
    /// use massa_models::operation::{OperationPrefixId, OperationPrefixIds, OperationPrefixIdsSerializer, OPERATION_ID_PREFIX_SIZE_BYTES};
    /// use massa_serialization::Serializer;
    ///
    /// let mut op_prefixes = OperationPrefixIds::default();
    /// op_prefixes.insert(OperationPrefixId::from(&[20; OPERATION_ID_PREFIX_SIZE_BYTES]));
    /// op_prefixes.insert(OperationPrefixId::from(&[20; OPERATION_ID_PREFIX_SIZE_BYTES]));
    /// let mut buffer = Vec::new();
    /// OperationPrefixIdsSerializer::new().serialize(&op_prefixes, &mut buffer).unwrap();
    /// ```
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
    length_deserializer: U32VarIntDeserializer,
    pref_deserializer: OperationPrefixIdDeserializer,
}

impl OperationPrefixIdsDeserializer {
    /// Creates a new `OperationIdsDeserializer`
    pub const fn new(max_operations_per_message: u32) -> Self {
        Self {
            length_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(max_operations_per_message),
            ),
            pref_deserializer: OperationPrefixIdDeserializer::new(),
        }
    }
}

impl Deserializer<OperationPrefixIds> for OperationPrefixIdsDeserializer {
    /// ## Example:
    /// ```rust
    /// use massa_models::{operation::{OperationPrefixId, OperationPrefixIds, OperationPrefixIdsSerializer, OperationPrefixIdsDeserializer, OPERATION_ID_PREFIX_SIZE_BYTES}};
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    ///
    /// let mut op_prefixes = OperationPrefixIds::default();
    /// op_prefixes.insert(OperationPrefixId::from(&[20; OPERATION_ID_PREFIX_SIZE_BYTES]));
    /// op_prefixes.insert(OperationPrefixId::from(&[20; OPERATION_ID_PREFIX_SIZE_BYTES]));
    /// let mut buffer = Vec::new();
    /// OperationPrefixIdsSerializer::new().serialize(&op_prefixes, &mut buffer).unwrap();
    /// let (rest, deserialized) = OperationPrefixIdsDeserializer::new(1000).deserialize::<DeserializeError>(&buffer).unwrap();
    /// assert_eq!(rest.len(), 0);
    /// assert_eq!(deserialized, op_prefixes);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], OperationPrefixIds, E> {
        context(
            "Failed OperationPrefixIds deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.length_deserializer.deserialize(input)
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
            SerializeError::NumberTooBig(
                "could not encode Set<OperationId> list length as u32".into(),
            )
        })?;
        self.u32_serializer.serialize(&list_len, buffer)?;
        for prefix in value {
            buffer.extend(Vec::<u8>::from(prefix));
        }
        Ok(())
    }
}

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

impl Serializer<Vec<WrappedOperation>> for OperationsSerializer {
    /// ## Example:
    /// ```rust
    /// use massa_models::{operation::{WrappedOperation, Operation, OperationType, OperationsSerializer, OperationSerializer}, wrapped::WrappedContent, address::Address, amount::Amount};
    /// use massa_signature::KeyPair;
    /// use massa_serialization::Serializer;
    /// use std::str::FromStr;
    ///
    /// let keypair = KeyPair::generate();
    /// let op = OperationType::Transaction {
    ///    recipient_address: Address::from_public_key(&keypair.get_public_key()),
    ///    amount: Amount::from_str("300").unwrap(),
    /// };
    /// let content = Operation {
    ///   fee: Amount::from_str("20").unwrap(),
    ///   op,
    ///   expire_period: 50,
    /// };
    /// let op_wrapped = Operation::new_wrapped(content, OperationSerializer::new(), &keypair).unwrap();
    /// let operations = vec![op_wrapped.clone(), op_wrapped.clone()];
    /// let mut buffer = Vec::new();
    /// OperationsSerializer::new().serialize(&operations, &mut buffer).unwrap();
    /// ```
    fn serialize(
        &self,
        value: &Vec<WrappedOperation>,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
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
    length_deserializer: U32VarIntDeserializer,
    signed_op_deserializer: WrappedDeserializer<Operation, OperationDeserializer>,
}

impl OperationsDeserializer {
    /// Creates a new `OperationsDeserializer`
    pub fn new(
        max_operations_per_message: u32,
        max_datastore_value_length: u64,
        max_function_name_length: u16,
        max_parameters_size: u32,
        max_op_datastore_entry_count: u64,
        max_op_datastore_key_length: u8,
        max_op_datastore_value_length: u64,
    ) -> Self {
        Self {
            length_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(max_operations_per_message),
            ),
            signed_op_deserializer: WrappedDeserializer::new(OperationDeserializer::new(
                max_datastore_value_length,
                max_function_name_length,
                max_parameters_size,
                max_op_datastore_entry_count,
                max_op_datastore_key_length,
                max_op_datastore_value_length,
            )),
        }
    }
}

impl Deserializer<Vec<WrappedOperation>> for OperationsDeserializer {
    /// ## Example:
    /// ```rust
    /// use massa_models::{operation::{WrappedOperation, Operation, OperationType, OperationsSerializer, OperationsDeserializer, OperationSerializer}, wrapped::WrappedContent, address::Address, amount::Amount};
    /// use massa_signature::KeyPair;
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use std::str::FromStr;
    ///
    /// let keypair = KeyPair::generate();
    /// let op = OperationType::Transaction {
    ///    recipient_address: Address::from_public_key(&keypair.get_public_key()),
    ///    amount: Amount::from_str("300").unwrap(),
    /// };
    /// let content = Operation {
    ///   fee: Amount::from_str("20").unwrap(),
    ///   op,
    ///   expire_period: 50,
    /// };
    /// let op_wrapped = Operation::new_wrapped(content, OperationSerializer::new(), &keypair).unwrap();
    /// let operations = vec![op_wrapped.clone(), op_wrapped.clone()];
    /// let mut buffer = Vec::new();
    /// OperationsSerializer::new().serialize(&operations, &mut buffer).unwrap();
    /// let (rest, deserialized_operations) = OperationsDeserializer::new(10000, 10000, 10000, 10000, 10, 255, 10_000).deserialize::<DeserializeError>(&buffer).unwrap();
    /// for (operation1, operation2) in deserialized_operations.iter().zip(operations.iter()) {
    ///     assert_eq!(operation1.id, operation2.id);
    ///     assert_eq!(operation1.signature, operation2.signature);
    ///     assert_eq!(operation1.creator_public_key, operation2.creator_public_key);
    ///     assert_eq!(operation1.content.fee, operation2.content.fee);
    /// }
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Vec<WrappedOperation>, E> {
        context(
            "Failed Operations deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.length_deserializer.deserialize(input)
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
    use crate::config::{
        MAX_DATASTORE_VALUE_LENGTH, MAX_FUNCTION_NAME_LENGTH, MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        MAX_OPERATION_DATASTORE_KEY_LENGTH, MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        MAX_PARAMETERS_SIZE,
    };

    use super::*;
    use massa_serialization::DeserializeError;
    use massa_signature::KeyPair;
    use serial_test::serial;
    use std::collections::BTreeMap;

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
        let (_, res_type) = OperationTypeDeserializer::new(
            MAX_DATASTORE_VALUE_LENGTH,
            MAX_FUNCTION_NAME_LENGTH,
            MAX_PARAMETERS_SIZE,
            MAX_OPERATION_DATASTORE_ENTRY_COUNT,
            MAX_OPERATION_DATASTORE_KEY_LENGTH,
            MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        )
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
        let (_, res_content) = OperationDeserializer::new(
            MAX_DATASTORE_VALUE_LENGTH,
            MAX_FUNCTION_NAME_LENGTH,
            MAX_PARAMETERS_SIZE,
            MAX_OPERATION_DATASTORE_ENTRY_COUNT,
            MAX_OPERATION_DATASTORE_KEY_LENGTH,
            MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        )
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
            WrappedDeserializer::new(OperationDeserializer::new(
                MAX_DATASTORE_VALUE_LENGTH,
                MAX_FUNCTION_NAME_LENGTH,
                MAX_PARAMETERS_SIZE,
                MAX_OPERATION_DATASTORE_ENTRY_COUNT,
                MAX_OPERATION_DATASTORE_KEY_LENGTH,
                MAX_OPERATION_DATASTORE_VALUE_LENGTH,
            ))
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
            gas_price: Amount::from_str("772.122").unwrap(),
            data: vec![23u8, 123u8, 44u8],
            datastore: BTreeMap::from([
                (vec![1, 2, 3], vec![4, 5, 6, 7, 8, 9]),
                (vec![22, 33, 44, 55, 66, 77], vec![11]),
            ]),
        };
        let mut ser_type = Vec::new();
        OperationTypeSerializer::new()
            .serialize(&op, &mut ser_type)
            .unwrap();
        let (_, res_type) = OperationTypeDeserializer::new(
            MAX_DATASTORE_VALUE_LENGTH,
            MAX_FUNCTION_NAME_LENGTH,
            MAX_PARAMETERS_SIZE,
            MAX_OPERATION_DATASTORE_ENTRY_COUNT,
            MAX_OPERATION_DATASTORE_KEY_LENGTH,
            MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        )
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
        let (_, res_content) = OperationDeserializer::new(
            MAX_DATASTORE_VALUE_LENGTH,
            MAX_FUNCTION_NAME_LENGTH,
            MAX_PARAMETERS_SIZE,
            MAX_OPERATION_DATASTORE_ENTRY_COUNT,
            MAX_OPERATION_DATASTORE_KEY_LENGTH,
            MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        )
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
            WrappedDeserializer::new(OperationDeserializer::new(
                MAX_DATASTORE_VALUE_LENGTH,
                MAX_FUNCTION_NAME_LENGTH,
                MAX_PARAMETERS_SIZE,
                MAX_OPERATION_DATASTORE_ENTRY_COUNT,
                MAX_OPERATION_DATASTORE_KEY_LENGTH,
                MAX_OPERATION_DATASTORE_VALUE_LENGTH,
            ))
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
            coins: Amount::from_str("456.789").unwrap(),
            gas_price: Amount::from_str("772.122").unwrap(),
            target_func: "target function".to_string(),
            param: "parameter".to_string(),
        };
        let mut ser_type = Vec::new();
        OperationTypeSerializer::new()
            .serialize(&op, &mut ser_type)
            .unwrap();
        let (_, res_type) = OperationTypeDeserializer::new(
            MAX_DATASTORE_VALUE_LENGTH,
            MAX_FUNCTION_NAME_LENGTH,
            MAX_PARAMETERS_SIZE,
            MAX_OPERATION_DATASTORE_ENTRY_COUNT,
            MAX_OPERATION_DATASTORE_KEY_LENGTH,
            MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        )
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
        let (_, res_content) = OperationDeserializer::new(
            MAX_DATASTORE_VALUE_LENGTH,
            MAX_FUNCTION_NAME_LENGTH,
            MAX_PARAMETERS_SIZE,
            MAX_OPERATION_DATASTORE_ENTRY_COUNT,
            MAX_OPERATION_DATASTORE_KEY_LENGTH,
            MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        )
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
            WrappedDeserializer::new(OperationDeserializer::new(
                MAX_DATASTORE_VALUE_LENGTH,
                MAX_FUNCTION_NAME_LENGTH,
                MAX_PARAMETERS_SIZE,
                MAX_OPERATION_DATASTORE_ENTRY_COUNT,
                MAX_OPERATION_DATASTORE_KEY_LENGTH,
                MAX_OPERATION_DATASTORE_VALUE_LENGTH,
            ))
            .deserialize::<DeserializeError>(&ser_op)
            .unwrap();
        assert_eq!(format!("{}", res_op), format!("{}", op));

        assert_eq!(op.get_validity_range(10), 40..=50);
    }
}
