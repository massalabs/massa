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

### Speculative roll registry

Have a speculative roll registry with:


* record production statistics (nb draws vs nb finalized blocks and endorsements) of all involved addresses for every cycle

* have the proof of stake state be included into massa-state (with bootstrap)

* store the snapshots and seeds of the last 5 cycles



* when applying a roll buy:
  * check for compensable roll sales in the cycle (full reimbursement) to substract from
  * ensure that there are enough coins in the sequential balance to buy the remaining coins
  * substract the coins from the seq balance from the address
  * increment the number of bought rolls in the

* for draws at cycle C:
  * take the distrib from the snapshot of C-3
  * take the RNG seed from the snapshot of C-2

* after the last slot of cycle C finalizes:
  * apply implicit roll sales according to cycle C production stats
  * apply partial or full reimbursement of coins locked at cycle C-1
  * save the snapshot of the distribution
  * save the snapshot of the seed
  * save cycle C production stats
  * 
   



* 
