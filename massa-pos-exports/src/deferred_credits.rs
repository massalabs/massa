use massa_models::{
    address::{Address, AddressDeserializer},
    amount::{Amount, AmountDeserializer, AmountSerializer},
    prehash::PreHashMap,
    slot::{Slot, SlotDeserializer, SlotSerializer},
};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};
use std::ops::Bound::{Excluded, Included, Unbounded};
use std::{collections::BTreeMap, ops::Bound};

#[derive(Debug, Default, Clone)]
/// Structure containing all the PoS deferred credits information
pub struct DeferredCredits(pub BTreeMap<Slot, PreHashMap<Address, Amount>>);

impl DeferredCredits {
    /// Extends the current `DeferredCredits` with another but accumulates the addresses and amounts
    pub fn nested_extend(&mut self, other: Self) {
        for (slot, new_credits) in other.0 {
            self.0
                .entry(slot)
                .and_modify(|current_credits| {
                    for (address, new_amount) in new_credits.iter() {
                        current_credits
                            .entry(*address)
                            .and_modify(|current_amount| {
                                *current_amount = current_amount.saturating_add(*new_amount);
                            })
                            .or_insert(*new_amount);
                    }
                })
                .or_insert(new_credits);
        }
    }

    /// Remove zero credits
    pub fn remove_zeros(&mut self) {
        let mut delete_slots = Vec::new();
        for (slot, credits) in &mut self.0 {
            credits.retain(|_addr, amount| !amount.is_zero());
            if credits.is_empty() {
                delete_slots.push(*slot);
            }
        }
        for slot in delete_slots {
            self.0.remove(&slot);
        }
    }
}

/// Serializer for `DeferredCredits`
pub struct DeferredCreditsSerializer {
    slot_ser: SlotSerializer,
    u64_ser: U64VarIntSerializer,
    amount_ser: AmountSerializer,
    cursor: Bound<Slot>,
}

impl DeferredCreditsSerializer {
    /// Creates a new `DeferredCredits` serializer
    pub fn new(cursor: Bound<Slot>) -> Self {
        Self {
            slot_ser: SlotSerializer::new(),
            u64_ser: U64VarIntSerializer::new(),
            amount_ser: AmountSerializer::new(),
            cursor,
        }
    }
}

impl Serializer<DeferredCredits> for DeferredCreditsSerializer {
    fn serialize(
        &self,
        value: &DeferredCredits,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        let range = value.0.range((self.cursor, Unbounded));
        // deferred credits given range length
        if range.clone().last().is_some() {
            self.u64_ser
                .serialize(&(range.clone().count() as u64), buffer)?;
        }
        // deferred credits
        for (slot, credits) in range.clone() {
            // slot
            self.slot_ser.serialize(slot, buffer)?;
            // slot credits length
            self.u64_ser.serialize(&(credits.len() as u64), buffer)?;
            // slot credits
            for (addr, amount) in credits {
                // address
                buffer.extend(addr.to_bytes());
                // credited amount
                self.amount_ser.serialize(amount, buffer)?;
            }
        }
        Ok(())
    }
}

/// Deserializer for `DeferredCredits`
pub struct DeferredCreditsDeserializer {
    u64_deserializer: U64VarIntDeserializer,
    slot_deserializer: SlotDeserializer,
    credit_deserializer: CreditDeserializer,
}

impl DeferredCreditsDeserializer {
    /// Creates a new `DeferredCredits` deserializer
    pub fn new(thread_count: u8) -> DeferredCreditsDeserializer {
        DeferredCreditsDeserializer {
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            credit_deserializer: CreditDeserializer::new(),
        }
    }
}

impl Deserializer<DeferredCredits> for DeferredCreditsDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], DeferredCredits, E> {
        context(
            "Failed DeferredCredits deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.u64_deserializer.deserialize(input)
                }),
                tuple((
                    |input| self.slot_deserializer.deserialize(input),
                    |input| self.credit_deserializer.deserialize(input),
                )),
            ),
        )
        .map(|elements| DeferredCredits(elements.into_iter().collect()))
        .parse(buffer)
    }
}

/// Deserializer for a single credit
struct CreditDeserializer {
    u64_deserializer: U64VarIntDeserializer,
    address_deserializer: AddressDeserializer,
    amount_deserializer: AmountDeserializer,
}

impl CreditDeserializer {
    /// Creates a new single credit deserializer
    fn new() -> CreditDeserializer {
        CreditDeserializer {
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
            address_deserializer: AddressDeserializer::new(),
            amount_deserializer: AmountDeserializer::new(
                Included(Amount::MIN),
                Included(Amount::MAX),
            ),
        }
    }
}

impl Deserializer<PreHashMap<Address, Amount>> for CreditDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], PreHashMap<Address, Amount>, E> {
        context(
            "Failed Credit deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.u64_deserializer.deserialize(input)
                }),
                tuple((
                    |input| self.address_deserializer.deserialize(input),
                    |input| self.amount_deserializer.deserialize(input),
                )),
            ),
        )
        .map(|elements| elements.into_iter().collect())
        .parse(buffer)
    }
}
