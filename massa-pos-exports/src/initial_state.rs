use massa_hash::{Hash, HashDeserializer, HashSerializer};
use massa_models::address::{Address, AddressSerializer};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{error::context, multi::length_count, sequence::tuple, IResult, Parser};
use std::collections::BTreeMap;
use std::ops::Bound::Included;

use crate::RollsDeserializer;

/// Initial state of the pos state, used for look back before genesis
#[derive(Debug, Clone, PartialEq)]
pub struct InitialState {
    /// initial rolls, used for negative cycle look back
    pub initial_rolls: BTreeMap<Address, u64>,
    /// initial seeds, used for negative cycle look back (cycles -2, -1 in that order)
    pub initial_seeds: Vec<Hash>,
    /// initial state hash
    pub initial_ledger_hash: Hash,
    /// initial cycle
    pub initial_cycle: u64,
    /// last start period
    pub last_start_period: u64,
}

/// Serializer for `InitialState`
pub struct InitialStateSerializer {
    u64_serializer: U64VarIntSerializer,
    address_serializer: AddressSerializer,
    hash_serializer: HashSerializer,
}

impl Default for InitialStateSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl InitialStateSerializer {
    /// Initialize a `InitialStateSerializer`
    pub fn new() -> Self {
        Self {
            u64_serializer: U64VarIntSerializer::new(),
            address_serializer: AddressSerializer::new(),
            hash_serializer: HashSerializer::new(),
        }
    }
}

impl Serializer<InitialState> for InitialStateSerializer {
    fn serialize(&self, value: &InitialState, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.u64_serializer
            .serialize(&(value.initial_rolls.len() as u64), buffer)?;
        for (addr, count) in &value.initial_rolls {
            self.address_serializer.serialize(addr, buffer)?;
            self.u64_serializer.serialize(count, buffer)?;
        }
        self.u64_serializer
            .serialize(&(value.initial_seeds.len() as u64), buffer)?;
        for seed in &value.initial_seeds {
            self.hash_serializer.serialize(seed, buffer)?;
        }
        self.hash_serializer
            .serialize(&value.initial_ledger_hash, buffer)?;
        self.u64_serializer
            .serialize(&value.initial_cycle, buffer)?;
        self.u64_serializer
            .serialize(&value.last_start_period, buffer)?;

        Ok(())
    }
}

/// Deserializer for `InitialState`
pub struct InitialStateDeserializer {
    u64_deserializer: U64VarIntDeserializer,
    hash_deserializer: HashDeserializer,
    rolls_deserializer: RollsDeserializer,
}

impl InitialStateDeserializer {
    /// Initialize a`InitialStateDeserializer`
    pub fn new(max_rolls_length: u64) -> Self {
        Self {
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
            hash_deserializer: HashDeserializer::new(),
            rolls_deserializer: RollsDeserializer::new(max_rolls_length),
        }
    }
}

impl Deserializer<InitialState> for InitialStateDeserializer {
    fn deserialize<'a, E: nom::error::ParseError<&'a [u8]> + nom::error::ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], InitialState, E> {
        context("Failed InitialState deserialization", |buffer| {
            tuple((
                context("Failed initial_rolls deserialization", |input| {
                    self.rolls_deserializer.deserialize(input)
                }),
                context(
                    "Failed initial_seeds deserialization",
                    length_count(
                        context("Failed length deserialization", |input| {
                            self.u64_deserializer.deserialize(input)
                        }),
                        context("Failed seeds deserialization", |input| {
                            self.hash_deserializer.deserialize(input)
                        }),
                    ),
                ),
                context("Failed initial_ledger_hash deserialization", |input| {
                    self.hash_deserializer.deserialize(input)
                }),
                context("Failed initial_cycle deserialization", |input| {
                    self.u64_deserializer.deserialize(input)
                }),
                context("Failed last_start_period deserialization", |input| {
                    self.u64_deserializer.deserialize(input)
                }),
            ))
            .map(
                |(
                    initial_rolls,
                    initial_seeds,
                    initial_ledger_hash,
                    initial_cycle,
                    last_start_period,
                )| InitialState {
                    initial_rolls: initial_rolls.into_iter().collect(),
                    initial_seeds,
                    initial_ledger_hash,
                    initial_cycle,
                    last_start_period,
                },
            )
            .parse(buffer)
        })
        .parse(buffer)
    }
}

#[cfg(test)]
mod test {
    use massa_serialization::DeserializeError;
    use massa_signature::KeyPair;

    use super::*;

    #[test]
    fn test_initial_state_ser_der() {
        let addr1 = Address::from_public_key(&KeyPair::generate().get_public_key());
        let addr2 = Address::from_public_key(&KeyPair::generate().get_public_key());

        let initial_state = InitialState {
            initial_rolls: BTreeMap::from([(addr1, 12), (addr2, 13)]),
            initial_seeds: vec![Hash::compute_from("123".as_bytes())],
            initial_ledger_hash: Hash::compute_from("123".as_bytes()),
            initial_cycle: 13,
            last_start_period: 12,
        };

        let initial_state_serializer = InitialStateSerializer::new();
        let initial_state_deserializer = InitialStateDeserializer::new(12);

        let mut buffer = Vec::new();

        initial_state_serializer
            .serialize(&initial_state, &mut buffer)
            .unwrap();

        let (rest, initial_state_ser_deser) = initial_state_deserializer
            .deserialize::<DeserializeError>(&buffer)
            .unwrap();

        assert!(rest.is_empty());
        assert_eq!(initial_state, initial_state_ser_deser);
    }
}
