use massa_models::{
    address::{Address, AddressDeserializer},
    amount::{Amount, AmountDeserializer},
    prehash::PreHashMap,
    slot::{Slot, SlotDeserializer},
};
use massa_serialization::{Deserializer, U64VarIntDeserializer};
use nom::{
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};
use std::collections::BTreeMap;
use std::ops::Bound::{Excluded, Included};

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
