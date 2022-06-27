// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::collections::{BTreeMap, VecDeque};

use bitvec::prelude::*;
use massa_models::{
    prehash::{Map, Set},
    Address, Amount, Slot,
};

pub struct PoSFinalState {
    /// contiguous cycle history. Front = newest.
    pub cycle_history: VecDeque<CycleInfo>,

    /// latest final slot
    pub last_final_slot: Slot,

    /// coins to be credited at the end of the slot
    pub deferred_credits: BTreeMap<Slot, Map<Address, Amount>>,
}

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

#[derive(Default, Debug, Copy, Clone)]
pub struct ProductionStats {
    pub block_success_count: u64,
    pub block_failure_count: u64,
}

#[derive(Default, Debug, Clone)]
pub struct PoSAddressInfo {
    /// set deferred credits indexed by target slot (can be set to 0 to cancel some, in case of slash)
    /// ordered structure to ensure slot iteration order is deterministic
    pub deferred_credits: BTreeMap<Slot, Amount>,

    /// updated production statistics
    pub production_stats: ProductionStats,

    /// new rolls counts
    pub roll_changes: u64,
}

#[derive(Default, Debug, Clone)]
pub struct PoSChanges {
    /// extra block seed bits added
    pub seed_bits: BitVec<Lsb0, u8>,

    /// addresses info
    pub addresses_info: Map<Address, PoSAddressInfo>,
}

#[derive(Clone)]
pub struct Selection {
    pub endorsments: Set<Address>,
    pub producer: Address,
}
