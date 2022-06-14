# Ledger unification

## General idea

Currently, there are two ledgers: the old sequential (sled) ledger managed by the consensus module, and the new massa-ledger (rocks db) managed by the execution module.
The goal here is to move the management of both ledgers into the execution module, and simplify consensus.

## Changes

* protocol should still verify operation signatures (invalidates block)
* consensus should still verify operation reuse (invalidates block)
* consensus should not check that operations execute properly anymore
* operation execution can fail but sequential balance fees will always be credited to the block producer 

## Proof-of-Stake

### Final PoS state in massa-final-state

```rust
struct PoSState {
  /// contiguous cycle history. Front = newest.
  cycle_history: VecDeque<CycleInfo>,

  /// latest final slot
  last_final_slot: Slot
}

struct CycleInfo {
  /// cycle number
  cycle: u64,
  
  /// whether the cycle is complete (all slots final)
  complete: bool,

  /// latest final slot of the cycle (if any)
  last_final_slot: Option<Slot>,

  /// number of rolls each staking address has
  roll_counts: BTreeMap<Address, u64>,

  /// random seed bits of all slots in the cycle so far
  rng_seed: BitVec<Lsb0, u8>,

  /// Per-address production statistics
  production_stats: Map<Address, ProductionStats>,

  /// coins to be credited at the end of the cycle
  deferred_credits: Map<Address, Amount>
}

/// NOTE: draws are done by the separate PoS module that is fed completed CycleInfos so that the draw algo is done in parallel, and shared so that all modules can efficiently read PoS draws

/// NOTE: when slashing, try to slash deferred credits first, then rolls. Otherwise attackers can do a roll sale and just after do an attack and not lose anything

struct ProductionStats {
  block_success_count: u64,
  block_failure_count: u64,
  endorsement_success_count: u64,
  endorsement_failure_count: u64
}
```

The `cycle_history` contains consecutive cycle stats, keeping the oldest one still needed in the `back()`, and the highest one needed (typically future cycles for deferred credits) in the `front()`.

`PoSFinalState` exposes a `settle_slot`

### Speculative PoS changes

In the speculative execution history, have:

```rust
struct PoSChanges {
  /// extra block seed bits added
  seed_bits: BitVec<Lsb0, u8>,

  /// new roll counts for addresses (can be 0 to remove the address from the registry)
  roll_changes: Map<Address, u64>

  /// updated production statistics
  production_stats: ProductionStats

  /// set deferred credits indexed by target slot (can be set to 0 to cancel some, in case of slash)
  deferred_credits: HashMap<Slot, Map<Address, Amount>>,
}

```

Possible lazy speculative queries:
* get roll count for a given address
  * used to check roll counts when buying a roll
  * used to set the absolute roll count in case of slashing (read the old state)
* list of all deferred credits for a given address after a certain slot
  * used to apply penalties by setting the new deferred credit (read the current incoming ones)
* list of all deferred credits for a given slot
  * used to apply the credits after the last slot of every cycle
* get prod stats to decide on implicit roll sales immediately after the last slot of a cycle

