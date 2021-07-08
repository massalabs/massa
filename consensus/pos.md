# Specs PoS

## In consensus

```
struct FinalRollThreadData {
    cycle: u64,
    last_final_slot: Slot,
    roll_count: BTreeMap<Address, u64>,  // number of rolls an address has
    cycle_purchases: HashMap<Address, u64>,  // compensated number of rolls an address has bought in the cycle
    cycle_sales: HashMap<Address, u64>,  // compenseated number of rolls an address has sold in the cycle
    rng_seed: BitVec // https://docs.rs/bitvec/0.22.3/bitvec/
}
```

* `last_final_block_cycle: Vec<u64>` indices: [thread] -> cycle index (u64)
* `final_roll_data: Vec< VecDeque<FinalRollThreadData> >` indices: [thread][cycle] -> FinalRollThreadData

Those values are either bootstrapped, or set to their genesis values:
* last_final_block_cycle = all zeros
* final_roll_data:
  * set the N=0 to the genesis roll distribution. Bitvec set to the 1st bit of the genesis block hashes as if they were the only final blocks here.
  * set the N=-1,-2,-3,-4 elements to genesis roll distribution. Bitvecs generated through an RNG seeded with the sha256 of a hardcoded seed in config.

## In ActiveBlock

```
struct RollThreadUpdates {  // subset of addresses whose rolls changed in a block
    roll_count: HashMap<Address, u64>,  // roll counts in the cycle so far for the involved addressses
    cycle_purchases: HashMap<Address, u64>, // compensated number of rolls an address has bought in the cycle so far for the involved addressses
    cycle_sales: HashMap<Address, u64> // compensated number of rolls an address has sold in the cycle so far for the involved addressses
}
```

Add a field:

* `roll_updates: RollThreadUpdates`  for the block's thread


## Draws

### Principle

This is part of the RandomSelector API.

To get the draws for cycle N, seed the RNG with the `Sha256(concatenation of FinalRollThreadData.rng_seed for threads from 0 to thread_count-1)`, then draw an index among the cumsum of the `final_roll_data.roll_count` for cycle `N-3` among all threads. There should be an error if `last_final_block_cycle + 3 <= N` for any thread.

### Cache

The selector system works on-demand: when the knwoledge of a block creator for a given slot is needed, it is returned if already in cache, otherwise:
* the cumsum vector and RNG seed are computed for the chosen cycle, and used to draw all the slots of the cycle which are stored in cache
* the oldest cached cycle draws are removed if the number of stored cycles > cfg.selector_max_cached_cycles

## When a new block B arrives in thread Tau, cycle N

### get_roll_data_at_parent

`get_roll_data_at_parent(BlockId, HashSet<Address>) -> RollThreadUpdates` returns the (current number of rolls, compensated number of purchases in cycle, compensated number of sales in cycle) addresses had at the output of BlockId.
This works by going backwards from BlockId (including BlockId) in the same thread parents.
* Loop, when a block Bp is reached:
  * if Bp is final:
    * get the missing address data from FinalRollThreadData
    * set cycle_purchases and cycle_sales to 0 if Bp is not in the same cycle as BlockId
    * fill up remainder addresses with zeros
    * break the loop
  * else:
    * if the address is in Bp's RollThreadUpdates, set its roll_count/cycle_purchases/cycle_sales to that values
    * set cycle_purchases and cycle_sales to 0 if Bp is not in the same cycle as BlockId
  * if all addresses are filled, break the loop



### block reception process

* check that the draw matches the block creator
* get the list of `roll_involved_addresses` buying/selling rolls in the block
* set `B.roll_updates = get_roll_data_at_parent(B.parents[Tau], roll_involved_addresses)`
* if the block is the first of a new cycle N for thread Tau:
  * credit `roll_price * final_roll_data[Tau][N-4].cycle_sales[addr]` for every addr in `final_roll_data[Tau][N-4].cycle_sales.keys()`
  * set all the compensated purchase/sale counts to 0 in `B.roll_update` (because they were counted for the previous cycle)
* parse the operations of the block in order:
  * if there is a ROLL_BUY[nb_rolls] operation from address A:
    * compute `sale_compensations = min(nb_rolls, B.roll_updates[A].cycle_sales)`
    * `B.roll_updates[A].cycle_sales -= sale_compensations`
    * `compensated_rolls = nb_rolls - sale_compensations`
    * apply `B.roll_updates[A].cycle_purchases += compensated_rolls`
    * try to apply ledgerchanges crediting the block creator with the fee, and substracting `compensated_rolls * config.roll_price` from the A's balance. If fails (low balance) => invalid.
    * `B.roll_updates[A].roll_count += compensated_rolls`
    
  * if there is a ROLL_SELL[nb_rolls] operation from address A:
    * compute `purchase_compensations = min(nb_rolls, B.roll_updates[A].cycle_purchases)`
    * `B.roll_updates[A].cycle_purchases -= purchase_compensations`
    * `compensated_rolls = nb_rolls - purchase_compensations`
    * credit A with `purchase_compensations * roll_price` tokens through ledgerchanges
    * apply `B.roll_updates[A].cycle_sales += compensated_rolls`
    * `B.roll_updates[A].roll_count -= compensated_rolls` => fail if negative    

## When a block B in thread Tau and cycle N becomes final

* if N > last_final_block_cycle[Tau]:
  * update last_final_block_cycle[Tau]
  * pop front for final_roll_data[thread] until the 1st element represents cycle N-4
  * push back a new last element in final_roll_data that represents cycle N:
    * inherit FinalRollThreadData.roll_count from cycle N-1
    * empty FinalRollThreadData.cycle_purchases, FinalRollThreadData.cycle_sales, FinalRollThreadData.rng_seed
* if there were misses between B and its parent, for each of them in order:
  * push the 1st bit of Sha256( miss.slot.to_bytes_key() ) in final_roll_data[thread].rng_seed
* push the 1st bit of BlockId in final_roll_data[thread].rng_seed
* overwrite the FinalRollThreadData sales/purchase entries at cycle N with the ones from the ActiveBlock 

