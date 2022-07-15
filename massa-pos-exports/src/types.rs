// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::collections::{BTreeMap, VecDeque};

use bitvec::prelude::*;
use massa_models::{
    constants::{POS_MISS_RATE_DEACTIVATION_THRESHOLD, THREAD_COUNT},
    prehash::Map,
    Address, Amount, AmountSerializer, Slot, SlotSerializer,
};
use massa_serialization::{SerializeError, Serializer, U64VarIntSerializer};
use num::rational::Ratio;
use std::ops::Bound::Included;

use crate::SelectorController;

/// Final state of PoS
#[derive(Default)]
pub struct PoSFinalState {
    /// contiguous cycle history. Front = newest.
    pub cycle_history: VecDeque<CycleInfo>,
    /// coins to be credited at the end of the slot
    pub deferred_credits: BTreeMap<Slot, Map<Address, Amount>>,
    /// selector controller to feed the cycle when completed
    pub selector: Option<Box<dyn SelectorController>>,
}

/// State of a cycle for all threads
#[derive(Default, Debug, Clone)]
pub struct CycleInfo {
    /// cycle number
    pub cycle: u64,
    /// whether the cycle is complete (all slots final)
    pub complete: bool,
    /// number of rolls each staking address has
    pub roll_counts: Map<Address, u64>,
    /// random seed bits of all slots in the cycle so far
    pub rng_seed: BitVec<Lsb0, u8>,
    /// Per-address production statistics
    pub production_stats: Map<Address, ProductionStats>,
}

/// Block production statistic
#[derive(Default, Debug, Copy, Clone)]
pub struct ProductionStats {
    /// Number of successfully created blocks
    pub block_success_count: u64,
    /// Number of blocks missed
    pub block_failure_count: u64,
}

impl ProductionStats {
    /// Check if the production stats are above the required percentage
    pub fn satisfying(&self) -> bool {
        let opportunities_count = self.block_success_count + self.block_failure_count;
        if opportunities_count == 0 {
            return true;
        }
        Ratio::new(self.block_failure_count, opportunities_count)
            <= *POS_MISS_RATE_DEACTIVATION_THRESHOLD
    }

    /// Increment a production stat struct with another
    pub fn chain(&mut self, stats: &ProductionStats) {
        self.block_success_count = self
            .block_success_count
            .saturating_add(stats.block_success_count);
        self.block_failure_count = self
            .block_failure_count
            .saturating_add(stats.block_failure_count);
    }
}

/// Recap of all PoS changes
#[derive(Default, Debug, Clone)]
pub struct PoSChanges {
    /// extra block seed bits added
    pub seed_bits: BitVec<Lsb0, u8>,

    /// new roll counts for addresses (can be 0 to remove the address from the registry)
    pub roll_changes: Map<Address, u64>,

    /// updated production statistics
    pub production_stats: Map<Address, ProductionStats>,

    /// set deferred credits indexed by target slot (can be set to 0 to cancel some, in case of slash)
    /// ordered structure to ensure slot iteration order is deterministic
    pub deferred_credits: BTreeMap<Slot, Map<Address, Amount>>,
}

/// DOC TODO
/// NOTE: address serialize is to_bytes
#[allow(dead_code)]
pub struct PoSChangesSerializer {
    u64_serializer: U64VarIntSerializer,
    slot_serializer: SlotSerializer,
    amount_serializer: AmountSerializer,
}

impl Default for PoSChangesSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl PoSChangesSerializer {
    /// DOC TODO
    pub fn new() -> PoSChangesSerializer {
        PoSChangesSerializer {
            u64_serializer: U64VarIntSerializer::new(Included(u64::MIN), Included(u64::MAX)),
            slot_serializer: SlotSerializer::new(
                (Included(u64::MIN), Included(u64::MAX)),
                (Included(0), Included(THREAD_COUNT)),
            ),
            amount_serializer: AmountSerializer::new(Included(u64::MIN), Included(u64::MAX)),
        }
    }
}

impl Serializer<PoSChanges> for PoSChangesSerializer {
    fn serialize(&self, value: &PoSChanges, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        let _roll_changes = {
            let entry_count: u64 = value.roll_changes.len().try_into().map_err(|err| {
                SerializeError::GeneralError(format!("too many entries in roll_changes: {}", err))
            })?;
            self.u64_serializer.serialize(&entry_count, buffer)?;
            for (addr, roll) in value.roll_changes.iter() {
                buffer.extend(addr.to_bytes());
                self.u64_serializer.serialize(roll, buffer)?;
            }
        };
        let _production_stats = {
            let entry_count: u64 = value.production_stats.len().try_into().map_err(|err| {
                SerializeError::GeneralError(format!(
                    "too many entries in production_stats: {}",
                    err
                ))
            })?;
            self.u64_serializer.serialize(&entry_count, buffer)?;
            for (
                addr,
                ProductionStats {
                    block_success_count,
                    block_failure_count,
                },
            ) in value.production_stats.iter()
            {
                buffer.extend(addr.to_bytes());
                self.u64_serializer.serialize(block_success_count, buffer)?;
                self.u64_serializer.serialize(block_failure_count, buffer)?;
            }
        };
        let _deferred_credits = {
            let entry_count: u64 = value.production_stats.len().try_into().map_err(|err| {
                SerializeError::GeneralError(format!(
                    "too many entries in deferred_credits: {}",
                    err
                ))
            })?;
            self.u64_serializer.serialize(&entry_count, buffer)?;
            for (slot, credits) in value.deferred_credits.iter() {
                self.slot_serializer.serialize(slot, buffer)?;
                for (addr, amount) in credits {
                    buffer.extend(addr.to_bytes());
                    self.amount_serializer.serialize(amount, buffer)?;
                }
            }
        };
        Ok(())
    }
}

/// Selections of endorsements and producer
#[derive(Clone)]
pub struct Selection {
    /// Choosen endorsements
    pub endorsments: Vec<Address>,
    /// Choosen block producer
    pub producer: Address,
}
