// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::collections::{BTreeMap, VecDeque};

use bitvec::prelude::*;
use massa_models::{prehash::Map, Address, Amount, Slot};

/// Final state of PoS
pub struct PoSFinalState {
    /// contiguous cycle history. Front = newest.
    pub cycle_history: VecDeque<CycleInfo>,
    /// latest final slot
    pub last_final_slot: Slot,
    /// coins to be credited at the end of the slot
    pub deferred_credits: BTreeMap<Slot, Map<Address, Amount>>,
}

/// State of a cycle for all threads
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
pub struct ProductionStats {
    /// Number of successfully created blocks
    pub block_success_count: u64,
    /// Number of blocks missed
    pub block_failure_count: u64,
}

/// Recap of all PoS changes
pub struct PoSChanges {
    /// extra block seed bits added
    pub seed_bits: BitVec<Lsb0, u8>,
    /// new roll counts for addresses (can be 0 to remove the address from the registry)
    pub roll_changes: Map<Address, u64>,
    /// updated production statistics
    pub production_stats: ProductionStats,
    /// set deferred credits indexed by target slot (can be set to 0 to cancel some, in case of slash)
    /// ordered structure to ensure slot iteration order is deterministic
    pub deferred_credits: BTreeMap<Slot, Map<Address, Amount>>,
}

/// Selections of endorsements and producer
#[derive(Clone)]
pub struct Selection {
    /// Choosen endorsements
    pub endorsments: Vec<Address>,
    /// Choosen block producer
    pub producer: Address,
}
