# Specs PoS

## In consensus

```
pub struct RollUpdate {
    pub roll_increment: bool, // true = majority buy, false = majority sell
    pub roll_delta: u64,      // absolute change in roll count
}

pub struct RollUpdates(pub HashMap<Address, RollUpdate>);

struct ThreadCycleState {
    cycle: u64,
    last_final_slot: Slot,  // last final slot of cycle for this thread (even if miss)
    roll_count: RollCounts,  // number of rolls addresses have
    cycle_updates: RollUpdates,  // compensated number of rolls addresses have bought/sold in the cycle 
    rng_seed: BitVec // https://docs.rs/bitvec/0.22.3/bitvec/
}

pub struct RollCounts(pub BTreeMap<Address, u64>);
```

* `cycle_states: Vec<VecDeque<ThreadCycleState> >` indices: [thread][cycle] -> ThreadCycleState

Those values are either bootstrapped, or set to their genesis values:
* cycle_states:
  * set the N=0 to the genesis roll distribution. Bitvec set to the 1st bit of the genesis block hashes as if they were the only final blocks here.
  * set the N=-1,-2,-3,-4 elements to genesis roll distribution. Bitvecs generated through an RNG seeded with the sha256 of a hardcoded seed in config.

## In ActiveBlock

Add a field:

* `roll_updates: RollUpdates`  for the block's thread


## Draws

### Principle

To get the draws for cycle N, seed the RNG with the `Sha256(concatenation of ThreadCycleState.rng_seed for all threads in increasing order)`, then draw an index among the cumsum of the `cycle_states.roll_count` for cycle `N-3` among all threads. THe complexity of a draw should be in `O(log(K))` where K is the roll ledger size. 

### Special case: if N-3 < 0

If the lookback aims at a cycle `-N` below zero, use:
* the initial roll registry as the roll count source
* `sha256^N(cfg.initial_draw_seed)` as the seed, where `sha256^N` means that we apply the sha256 hash function N times consecutively
### Special case: genesis blocks

For genesis blocks, force the draw to yield the genesis creator's address.
### Cache

When computing draws for a cycle, draw the whole cycle and leave it in cache.
Keep a config-defined maximal number of cycles in cache.
If there are too many, drop the ones with the lowest sequence number.
Whenever cache is computed or read for a given cycle, set its sequence number to a new incremental value.

## When a new block B arrives in thread Tau, cycle N

### get_roll_data_at_parent

`get_roll_data_at_parent(BlockId, HashSet<Address>) -> RollCounts` returns the RollCounts addresses had at the output of BlockId.

Algo:
* start from BlockId, and explore itself and its ancestors backwars in the same thread:
  * if a final block is explored, break + save final_cycle 
  * otherwise, stack the explored block ID
* set cur_rolls = the latest final roll state at final_cycle for the selected addresses
* while block_id = stack.pop():
  * apply active_block[block_id].roll_updates to cur_rolls
* return cur_rolls

### block reception process

* check that the draw matches the block creator
* get the list of `roll_involved_addresses` buying/selling rolls in the block
* set `cur_rolls = get_roll_data_at_parent(B.parents[Tau], roll_involved_addresses)`
* set `B.roll_updates = new()`
* if the block is the first of a new cycle N for thread Tau:
  * credit `roll_price * cycle_states[Tau][4].roll_updates[addr].roll_delta` for every addr for which `roll_increment == false`
* parse the operations of the block in order:
  * try apply roll changes to cur_rolls. If failure => block invalid
  * try chain roll changes to `B.roll_updates`. If failure => block invalid

## When a block B in thread Tau and cycle N becomes final

* step 1:
  * if N > cycle_states[thread][0].cycle:
    * push front a new element in cycle_states that represents cycle N:
      * inherit ThreadCycleState.roll_count from cycle N-1
      * empty ThreadCycleState.cycle_updates, ThreadCycleState.rng_seed
    * pop back for cycle_states[thread] to keep it the right size
* step 2:
  * if there were misses between B and its parent, for each of them in order:
    * push the 1st bit of Sha256( miss.slot.to_bytes_key() ) in cycle_states[thread].rng_seed
  * push the 1st bit of BlockId in cycle_states[thread].rng_seed
  * update the ThreadCycleState roll counts at cycle N with by applying ActiveBlock.roll_updates 
