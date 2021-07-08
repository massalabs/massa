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

To get the draws for cycle N, seed the RNG with the `Sha256(concatenation of ThreadCycleState.rng_seed for all threads in increasing order)`, then draw an index among the cumsum of the `cycle_states.roll_count` for cycle `N-3` among all threads.

> The complexity of a draw should be in `O(log(K))` where K is the roll ledger size.

* if the lookback cycle N-3 < 0:
  * use the initial (genesis) roll registry as the roll count source
  * `sha256^N(cfg.initial_draw_seed)` as the seed, where `sha256^N` means that we apply the sha256 hash function N times consecutively 

*  if the draw is being performed for a genesis slot, force the draw outcome to match the genesis creator's address to keep draws consistent

> For consistency, genesis block bits are also fed to the RNG seed bitfields

### Cache

When computing draws for a cycle, draw the whole cycle and leave it in cache.
Keep a config-defined maximal number of cycles in cache.
If there are too many, drop the ones with the lowest sequence number.
Whenever cache is computed or read for a given cycle, set its sequence number to a new incremental value.

## When a new block B arrives in thread Tau, cycle N

### get_roll_data_at_parent

`get_roll_data_at_parent(BlockId, HashSet<Address>) -> RollCounts` returns the RollCounts addresses had at the output of BlockId.

1. start from BlockId, and explore itself and its ancestors backwards in the same thread:
	1. if a final block is explored, break + save final_cycle 
	2. otherwise, stack the explored block ID
2. set cur_rolls = the latest final roll state at final_cycle for the selected addresses
3. while block_id = stack.pop():
	1. apply active_block[block_id].roll_updates to cur_rolls
4. return cur_rolls

### block reception process

1. check that the draw matches the block creator
2. get the list of `roll_involved_addresses` buying/selling rolls in the block
3. set `cur_rolls = get_roll_data_at_parent(B.parents[Tau], roll_involved_addresses)`
4. set `B.roll_updates = new()`
5. if the block is the first of a new cycle N for thread Tau:
	1. credit `roll_price * cycle_states[Tau][4].roll_updates[addr].roll_delta` for every addr for which `roll_increment == false`
6. parse the operations of the block in order:
	1. Apply roll changes to cur_rolls. If the new roll count under/over-flows u64 => block invalid
	2. try chain roll changes to `B.roll_updates`. If the new roll delta under/over-flows u64 => block invalid

## When a block B in thread Tau and cycle N becomes final

1. if N > cycle_states[thread][0].cycle:
	1. push front a new element in cycle_states that represents cycle N:
		1. inherit ThreadCycleState.roll_count from cycle N-1
		2. empty ThreadCycleState.cycle_updates, ThreadCycleState.rng_seed
	2. pop back for cycle_states[thread] to keep it the right size
2. if there were misses between B and its parent, for each of them in order:
	1. push the 1st bit of Sha256( miss.slot.to_bytes_key() ) in cycle_states[thread].rng_seed
3. update the ThreadCycleState roll counts at cycle N with by applying ActiveBlock.roll_updates 
4. push the 1st bit of BlockId in cycle_states[thread].rng_seed
