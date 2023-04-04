use massa_hash::{Hash, HASH_SIZE_BYTES};
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

const DEFERRED_CREDITS_HASH_INITIAL_BYTES: &[u8; 32] = &[0; HASH_SIZE_BYTES];

#[derive(Clone, Serialize, Deserialize)]
/// Structure containing all the PoS deferred credits information
pub struct DeferredCredits {
    /// Deferred credits
    pub credits: BTreeMap<Slot, PreHashMap<Address, Amount>>,
    /// Hash tracker, optional. Indeed, computing the hash is expensive, so we only compute it when finalizing a slot.
    #[serde(skip_serializing, skip_deserializing)]
    hash_tracker: Option<DeferredCreditsHashTracker>,
}

impl Debug for DeferredCredits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.credits)
    }
}

#[derive(Clone)]
struct DeferredCreditsHashTracker {
    slot_ser: SlotSerializer,
    address_ser: AddressSerializer,
    amount_ser: AmountSerializer,
    hash: Hash,
}

impl DeferredCreditsHashTracker {
    /// Initialize hash tracker
    fn new() -> Self {
        Self {
            slot_ser: SlotSerializer::new(),
            address_ser: AddressSerializer::new(),
            amount_ser: AmountSerializer::new(),
            hash: Hash::from_bytes(DEFERRED_CREDITS_HASH_INITIAL_BYTES),
        }
    }

    /// Get resulting hash from the tracker
    pub fn get_hash(&self) -> &Hash {
        &self.hash
    }

    /// Apply adding an element (must not be an overwrite) or deleting an element (must exist)
    pub fn toggle_entry(&mut self, slot: &Slot, address: &Address, amount: &Amount) {
        self.hash ^= self.compute_hash(slot, address, amount);
    }

    /// Compute the hash for a specific entry
    fn compute_hash(&self, slot: &Slot, address: &Address, amount: &Amount) -> Hash {
        // serialization can never fail in the following computations, unwrap is justified
        let mut buffer = Vec::new();
        self.slot_ser.serialize(slot, &mut buffer).unwrap();
        self.address_ser.serialize(address, &mut buffer).unwrap();
        self.amount_ser.serialize(amount, &mut buffer).unwrap();
        Hash::compute_from(&buffer)
    }
}

impl DeferredCredits {
    /// Check if the credits list is empty
    pub fn is_empty(&self) -> bool {
        self.credits.is_empty()
    }

    /// Create a new DeferredCredits with hash tracking
    pub fn new_with_hash() -> Self {
        Self {
            credits: Default::default(),
            hash_tracker: Some(DeferredCreditsHashTracker::new()),
        }
    }

    /// Create a new DeferredCredits without hash tracking
    pub fn new_without_hash() -> Self {
        Self {
            credits: Default::default(),
            hash_tracker: None,
        }
    }

    /// Get hash from tracker, if any
    pub fn get_hash(&self) -> Option<&Hash> {
        self.hash_tracker.as_ref().map(|ht| ht.get_hash())
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

    /// Enables the hash tracker (and compute the hash if absent)
    pub fn enable_hash_tracker_and_compute_hash(&mut self) -> &Hash {
        if self.hash_tracker.is_none() {
            let mut hash_tracker = DeferredCreditsHashTracker::new();
            self.for_each(|slot, address, amount| {
                hash_tracker.toggle_entry(slot, address, amount);
            });
            self.hash_tracker = Some(hash_tracker);
        }

        self.hash_tracker.as_ref().unwrap().get_hash()
    }

    /// Disable the hash tracker, loses hash
    pub fn disable_hash_tracker(&mut self) {
        self.hash_tracker = None;
    }

    /// Get all deferred credits within a slot range.
    /// If `with_hash == true` then the resulting DeferredCredits contains the hash of the included data.
    /// Note that computing the hash is heavy and should be done only at finalization.
    pub fn get_slot_range<R>(&self, range: R, with_hash: bool) -> DeferredCredits
    where
        R: RangeBounds<Slot>,
    {
        let mut res = DeferredCredits {
            credits: self
                .credits
                .range(range)
                .map(|(s, map)| (*s, map.clone()))
                .collect(),
            hash_tracker: None,
        };
        if with_hash {
            res.enable_hash_tracker_and_compute_hash();
        }
        res
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
            ref mut credits,
            ref mut hash_tracker,
            ..
        } = self;

        for (slot, credits) in credits {
            credits.retain(|address, amount| {
                // if amount is zero XOR the credit hash and do not retain
                if amount.is_zero() {
                    if let Some(ht) = hash_tracker.as_mut() {
                        ht.toggle_entry(slot, address, amount)
                    }
                    false
                } else {
                    true
                }
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
        let prev = self
            .credits
            .entry(slot)
            .or_default()
            .insert(address, amount);
        if let Some(ht) = &mut self.hash_tracker {
            if let Some(prev) = prev {
                // remove overwritten value
                ht.toggle_entry(&slot, &address, &prev);
            }
            ht.toggle_entry(&slot, &address, &amount);
        }
        prev
    }
}

/// Serializer for `DeferredCredits`
pub struct DeferredCreditsSerializer {
    slot_ser: SlotSerializer,
    u64_ser: U64VarIntSerializer,
    credits_ser: CreditsSerializer,
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

/// Deserializer for `DeferredCredits`
pub struct DeferredCreditsDeserializer {
    u64_deserializer: U64VarIntDeserializer,
    slot_deserializer: SlotDeserializer,
    credit_deserializer: CreditsDeserializer,
    enable_hash: bool,
}

impl DeferredCreditsDeserializer {
    /// Creates a new `DeferredCredits` deserializer
    pub fn new(
        thread_count: u8,
        max_credits_length: u64,
        enable_hash: bool,
    ) -> DeferredCreditsDeserializer {
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
            enable_hash,
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
        .map(|elements| {
            let mut res = DeferredCredits {
                credits: elements.into_iter().collect(),
                hash_tracker: None,
            };
            if self.enable_hash {
                res.enable_hash_tracker_and_compute_hash();
            }
            res
        })
        .parse(buffer)
    }
}

/// Serializer for `Credits`
pub struct CreditsSerializer {
    u64_ser: U64VarIntSerializer,
    address_ser: AddressSerializer,
    amount_ser: AmountSerializer,
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

/// Deserializer for a single credit
struct CreditsDeserializer {
    u64_deserializer: U64VarIntDeserializer,
    address_deserializer: AddressDeserializer,
    amount_deserializer: AmountDeserializer,
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
