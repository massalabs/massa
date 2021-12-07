# PoS

## In consensus

```ignore
pub struct RollCompensation(pub u64);

pub struct RollUpdate {
    pub roll_purchases: bool,
    pub roll_sales: u64,
}

impl RollUpdate {
	pub fn chain(&mut self, change: &Self) -> Result<RollCompensation, ConsensusError>;  // fuses Change into self and compensates + returns the number of matching purchases/sales
}

pub struct RollUpdates(pub HashMap<Address, RollUpdate>);

impl RollUpdates {
    // applies another RollUpdates to self, compensates and returns compensation counts for each compensated address
	// if addrs_opt is Some(addrs), restricts the changes to addrs
    pub fn chain_subset(
        &mut self,
        updates: &RollUpdates,
        addrs_opt: Option<&HashSet<Address>>,
    ) -> Result<HashMap<Address, RollCompensation>, ConsensusError>;

    // applies a RollUpdate to self, compensates and returns compensation count
    pub fn apply(&mut self, addr: &Address, update: &RollUpdate) -> Result<RollCompensation, ConsensusError>;
}

struct ThreadCycleState {
    cycle: u64,
    last_final_slot: Slot,  // last final slot of cycle for this thread (even if miss)
    roll_count: RollCounts,  // number of rolls addresses have
    cycle_updates: RollUpdates,  // compensated number of rolls addresses have bought/sold in the cycle
    rng_seed: BitVec // https://docs.rs/bitvec/0.22.3/bitvec/
    production_stats: HashMap<Address, (u64, u64)> // associates addresses to their (n_final_blocks, n_final_misses) in the cycle
}

pub struct RollCounts(pub BTreeMap<Address, u64>);


impl RollCounts {
    // applies RollUpdates to self with compensations
    // if addrs_opt is Some(addrs), the changes are restricted to addrs
    pub fn apply_subset(
        &mut self,
        updates: &RollUpdates,
        addrs_opt: Option<&HashSet<Address>>,
    ) -> Result<(), ConsensusError>;
}
```

-   `cycle_states: Vec<VecDeque<ThreadCycleState>>` indices: `[thread][cycle] -> ThreadCycleState`
    where the index "cycle" is relative cycle number with respect to the latest one stored.

Those values are either bootstrapped, or set to their genesis values:

-   cycle_states:
    -   set the N=0 to the genesis roll distribution. Bitvec set to the 1st bit of the genesis block hashes as if they were the only final blocks here.
    -   set the N=-1,-2,-3,-4 elements to genesis roll distribution. Bitvecs generated through an RNG seeded with the sha256 of a hardcoded seed in config.

## In ActiveBlock

Add a field:

-   `roll_updates: RollUpdates` for the block's thread
-   `production_history: Vec<(u64, Address, bool)>` block/endorsement production events (period, creator_address, true if produces or false if missed)

## Draws

### Principle

1. To get the draws for cycle N:
    1. seed the RNG with the `Sha256(concatenation of ThreadCycleState.rng_seed for all threads in increasing order)`,
    2. draw an index among the cumsum of the `cycle_states.roll_count` for cycle `N-3` among all threads.

> The complexity of a draw should be in `O(log(K))` where K is the roll ledger size.

1. if the lookback cycle N-3 < 0:
    1. use the initial (genesis) roll registry as the roll count source
    2. `sha256^N(cfg.initial_draw_seed)` as the seed, where `sha256^N` means that we apply the sha256 hash function N times consecutively
2. if the draw is being performed for a genesis slot
    1. force the draw outcome to match the genesis creator's address to keep draws consistent

> For consistency, genesis block bits are also fed to the RNG seed bitfields

### Cache

1. When computing draws for a cycle:
    1. draw the whole cycle and add to the cache.
    2. If the number of items in the cache is above a configured maximum:
        1. drop the ones with the lowest sequence number
2. Whenever cache is computed or read for a given cycle, set its sequence number to a new incremental value.

## When a new block B arrives in thread Tau, cycle N

### get_roll_data_at_parent

`get_roll_data_at_parent(BlockId, Option<HashSet<Address>>) -> (RollCounts, RollUpdates)` returns the RollCounts addresses had at the output of BlockId, as well as the cycle RollUpdates.
If the Option is None, all addresses are taken into account.

1. start from `BlockId`, and explore itself and its ancestors backwards in the same thread:
    1. if a final block is explored, break + save `final_cycle`
    2. otherwise, stack the explored block ID
2. set `cur_rolls` = the latest final roll state at `final_cycle` for the selected addresses
3. if `BlockId` is in the same thread as the latest final block, set `cur_updates` = the latest final cycle updates, otherwise empty updates
4. while `block_id == stack.pop()`:
    1. apply `active_block[block_id].roll_updates` to `cur_rolls`
    2. if `active_block[block_id].cycle == BlockId.cycle` => `apply active_block[block_id].roll_updates` to `cur_updates`
5. return `(cur_rolls, cur_updates)`

### block reception process

1. check that the draw matches the block creator
2. get the list of `roll_involved_addresses` buying/selling rolls in the block
3. set `(cur_rolls, cycle_roll_updates) = get_roll_data_at_parent(B.parents[Tau], roll_involved_addresses)`
    1. if the block's cycle is different from its parent's => empty cycle_roll_updates
4. set `B.roll_updates = new()`
5. if the block is the first of a new cycle N for thread Tau:
    1. credit `roll_price * cycle_states[Tau][1 + lookback_cycles + lock_cycles].roll_updates[addr].roll_delta` for every addr for which `roll_increment == false`
    2. deactivate all candidate rolls for all addresses for which `cycle_states[Tau][1 + lookback_cycles].production_stats[address].(.0 / (.0 + .1)) > cfg.pos_miss_rate_deactivation_threshold`
6. parse the operations of the block in order:
    1. Apply roll updates to cur_rolls. If the new roll count under/over-flows u64 => block invalid
    2. try chain roll updates to `B.roll_updates`. If the new roll delta under/over-flows u64 => block invalid
    3. try chain roll updates to `cycle_roll_updates`, credit the compensation roll count as ledger changes. If any error => block invalid
        1. try chain roll updates to `cycle_roll_updates` and gather compensation counts for each address. If error => block invalid.
        2. try chain roll changes to `block_ledger_changes` with compensation. If error => block invalid.

## When a block B in thread Tau and cycle N becomes final

1. if `N > cycle_states[thread][0].cycle`:
    1. push front a new element in cycle_states that represents cycle N:
        1. inherit `ThreadCycleState.roll_count` from cycle N-1
        2. empty `ThreadCycleState.cycle_updates`, `ThreadCycleState.rng_seed`
    2. pop back for `cycle_states[thread]` to keep it the right size
2. accumulate production statistics
3. if there were misses between B and its parent, for each of them in order:
    1. push the 1st bit of `Sha256( miss.slot.to_bytes_key() )` in `cycle_states[thread].rng_seed`
4. update the `ThreadCycleState` roll counts at cycle N with by applying `ActiveBlock.roll_updates`
5. push the 1st bit of `BlockId` in `cycle_states[thread].rng_seed`
