use massa_models::{
    address::{Address, AddressDeserializer, AddressSerializer},
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
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, ops::RangeBounds};
use std::{
    fmt::Debug,
    ops::Bound::{Excluded, Included},
};

#[derive(Clone, Serialize, Deserialize)]
/// Structure containing all the PoS deferred credits information
pub struct DeferredCredits {
    /// Deferred credits
    pub credits: BTreeMap<Slot, PreHashMap<Address, Amount>>,
}

impl Debug for DeferredCredits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.credits)
    }
}

impl Default for DeferredCredits {
    fn default() -> Self {
        Self::new()
    }
}

impl DeferredCredits {
    /// Check if the credits list is empty
    pub fn is_empty(&self) -> bool {
        self.credits.is_empty()
    }

    /// Create a new DeferredCredits with hash tracking
    pub fn new() -> Self {
        Self {
            credits: Default::default(),
        }
    }

    /// Apply a function to each element
    pub fn for_each<F>(&mut self, mut f: F)
    where
        F: FnMut(&Slot, &Address, &mut Amount),
    {
        for (slot, credits) in &mut self.credits {
            for (address, amount) in credits {
                f(slot, address, amount);
            }
        }
    }

    /// Get all deferred credits within a slot range.
    pub fn get_slot_range<R>(&self, range: R) -> DeferredCredits
    where
        R: RangeBounds<Slot>,
    {
        DeferredCredits {
            credits: self
                .credits
                .range(range)
                .map(|(s, map)| (*s, map.clone()))
                .collect(),
        }
    }

    /// Extends the current `DeferredCredits` with another and replace the amounts for existing addresses
    pub fn extend(&mut self, other: Self) {
        for (slot, credits) in other.credits {
            for (address, amount) in credits {
                self.insert(slot, address, amount);
            }
        }
    }

    /// Remove credits set to zero, use only on finality
    pub fn remove_zeros(&mut self) {
        let mut empty_slots = Vec::new();

        // We need to destructure self to be able to mutate both credits and the hash_tracker during iteration
        // Without it, the borrow-checker is angry that we try to mutate self twice.
        let Self {
            ref mut credits, ..
        } = self;

        for (slot, credits) in credits {
            credits.retain(|_, amount| {
                // do not retain if amount is zero
                !amount.is_zero()
            });
            if credits.is_empty() {
                empty_slots.push(*slot);
            }
        }
        for slot in empty_slots {
            self.credits.remove(&slot);
        }
    }

    /// Gets the deferred credits for a given address that will be credited at a given slot
    pub fn get_address_credits_for_slot(&self, addr: &Address, slot: &Slot) -> Option<Amount> {
        self.credits
            .get(slot)
            .and_then(|slot_credits| slot_credits.get(addr))
            .copied()
    }

    /// Insert an element
    pub fn insert(&mut self, slot: Slot, address: Address, amount: Amount) -> Option<Amount> {
        self.credits
            .entry(slot)
            .or_default()
            .insert(address, amount)
    }
}

#[derive(Clone)]
#[allow(missing_docs)]
/// Serializer for `DeferredCredits`
pub struct DeferredCreditsSerializer {
    pub slot_ser: SlotSerializer,
    pub u64_ser: U64VarIntSerializer,
    pub credits_ser: CreditsSerializer,
}

impl Default for DeferredCreditsSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl DeferredCreditsSerializer {
    /// Creates a new `DeferredCredits` serializer
    pub fn new() -> Self {
        Self {
            slot_ser: SlotSerializer::new(),
            u64_ser: U64VarIntSerializer::new(),
            credits_ser: CreditsSerializer::new(),
        }
    }
}

impl Serializer<DeferredCredits> for DeferredCreditsSerializer {
    fn serialize(
        &self,
        value: &DeferredCredits,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        // deferred credits length
        self.u64_ser
            .serialize(&(value.credits.len() as u64), buffer)?;
        // deferred credits
        for (slot, credits) in &value.credits {
            // slot
            self.slot_ser.serialize(slot, buffer)?;
            // credits
            self.credits_ser.serialize(credits, buffer)?;
        }
        Ok(())
    }
}

#[derive(Clone)]
#[allow(missing_docs)]
/// Deserializer for `DeferredCredits`
pub struct DeferredCreditsDeserializer {
    pub u64_deserializer: U64VarIntDeserializer,
    pub slot_deserializer: SlotDeserializer,
    pub credit_deserializer: CreditsDeserializer,
}

impl DeferredCreditsDeserializer {
    /// Creates a new `DeferredCredits` deserializer
    pub fn new(thread_count: u8, max_credits_length: u64) -> DeferredCreditsDeserializer {
        DeferredCreditsDeserializer {
            u64_deserializer: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(max_credits_length),
            ),
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            credit_deserializer: CreditsDeserializer::new(max_credits_length),
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
                    context("Failed slot deserialization", |input| {
                        self.slot_deserializer.deserialize(input)
                    }),
                    context("Failed credit deserialization", |input| {
                        self.credit_deserializer.deserialize(input)
                    }),
                )),
            ),
        )
        .map(|elements| DeferredCredits {
            credits: elements.into_iter().collect(),
        })
        .parse(buffer)
    }
}

#[derive(Clone)]
#[allow(missing_docs)]
/// Serializer for `Credits`
pub struct CreditsSerializer {
    pub u64_ser: U64VarIntSerializer,
    pub address_ser: AddressSerializer,
    pub amount_ser: AmountSerializer,
}

impl Default for CreditsSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl CreditsSerializer {
    /// Creates a new `Credits` serializer
    pub fn new() -> Self {
        Self {
            u64_ser: U64VarIntSerializer::new(),
            address_ser: AddressSerializer::new(),
            amount_ser: AmountSerializer::new(),
        }
    }
}

impl Serializer<PreHashMap<Address, Amount>> for CreditsSerializer {
    fn serialize(
        &self,
        value: &PreHashMap<Address, Amount>,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        // slot credits length
        self.u64_ser.serialize(&(value.len() as u64), buffer)?;
        // slot credits
        for (addr, amount) in value {
            // address
            self.address_ser.serialize(addr, buffer)?;
            // credited amount
            self.amount_ser.serialize(amount, buffer)?;
        }
        Ok(())
    }
}

#[derive(Clone)]
#[allow(missing_docs)]
/// Deserializer for a single credit
pub struct CreditsDeserializer {
    u64_deserializer: U64VarIntDeserializer,
    pub address_deserializer: AddressDeserializer,
    pub amount_deserializer: AmountDeserializer,
}

impl CreditsDeserializer {
    /// Creates a new single credit deserializer
    fn new(max_credits_length: u64) -> CreditsDeserializer {
        CreditsDeserializer {
            u64_deserializer: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(max_credits_length),
            ),
            address_deserializer: AddressDeserializer::new(),
            amount_deserializer: AmountDeserializer::new(
                Included(Amount::MIN),
                Included(Amount::MAX),
            ),
        }
    }
}

impl Deserializer<PreHashMap<Address, Amount>> for CreditsDeserializer {
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
                    context("Failed address deserialization", |input| {
                        self.address_deserializer.deserialize(input)
                    }),
                    context("Failed amount deserialization", |input| {
                        self.amount_deserializer.deserialize(input)
                    }),
                )),
            ),
        )
        .map(|elements| elements.into_iter().collect())
        .parse(buffer)
    }
}
