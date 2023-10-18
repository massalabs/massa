## Rationale

The `massa-factory` crate will be responsible for:
* maintaining the node's staking keys
* creating blocks
* creating endorsements

The factory module will function in separate, independent threads.

## Interface

### Inputs

The factory controller will allow acting on the factory from other modules:
* staking keys management: add, list, remove keys dynamically like it's done currently in consensus
* enable/disable block and endorsement production dynamically
* the API can query the factory to get production history and past/future draw slots and timestamps

### Outputs

The factory will need to call other modules:
* ask selector for draws
* ask consensus for best_parents and for blockclique blocks present in a given slot
* ask pool for operations and endorsements
* ask execution for the speculative balances of involved addresses, as well as previously executed op IDs
* send the created block/endorsement to storage
* send created block/endorsement to pool
* notify protocol of new endorsements to propagate
* notify pool/consensus of new block to take into account and propagate through protocol

### Clock

The factory module will maintain its own absolute clock based on `genesis_timestamp`, `t0` and `thread_count`.

## Block creation

### Timing

The block creation slot `(period, thread)` will be at time `genesis_timestamp + (t0*period) + ((thread*t0)/thread_count))`.

Since double-staking is penalized and could happen on node reboot, a safety measure is to wait 2 periods (`2*t0`) after the program starts before beginning block production.

We ignore block creation slots for which `S.period == 0` (genesis blocks).

At every block creation slot, the block creation thread will read selector draws at that slot to check whether one of the staking keys was selected to produce the block.
If the address linked to a stored staking key was selected, launch the block production process below.

### Block production

The production of a block `B` at slot `S` happens in steps:
* prepare the header content with the obvious fields of the header (eg. creator public key, slot)
* ask consensus for `best_parents` which will be the parents set inside the header
* ask pool for the endorsmeents to add to the header given the `BlockId` of `B`'s parent in `B`'s thread
* define `remaining_gas = MAX_BLOCk_GAS`  which is the remaining gas in the block
* define `remaining_space = MAX_BLOCK_SIZE` which is the remaining operation space in the block in bytes
* define `balance_cache: Map<Address, Amount> = Default::default()` which is a cache of balance
* define `excluded_ops: Set<OperationId>` which is the list of operations to exclude
* pre-fill`excluded_ops` by asking the `execution` execution module for the list of operations that have been executed previously in `B`'s thread
* define `start_time = MassaTime::now(0)` which is the time when we started producing the block
* loop:
  * if `MassaTime::now(0).saturating_sub(start_time) > MAX_BLOCK_PRODUCTION_MILLIS`, it means we have spent too much time in this loop => break loop
  * ask pool for a sorted batch of operations (best to worst) given the slot `S`, `remaining_gas`, `remaining_space`, `excluded_ops`
  * if the obtained batch is empty, break loop
  * extend `excluded_ops` with the obtained batch
  * for each operation `op` in the ordered batch:
    * if `op.get_gas_usage() > remaining_gas`, it means that there is not enough remaining block gas to execute the operation => continue loop
    * if `op.serialized_version.len() > remaining_space`, it means that there is not enough remaining space to include the operation => continue loop
    * if `op`'s creator address is not in `balance_cache`, ask `execution` for the balance of `op.creator` and insert it inside `balance_cache`
    * if `balance_cache[op.creator_addr] < op.fee + op.get_gas_coins()`, it means that the sender cannot pay for the fees => continue loop
    * add the operation to the block's operation list
    * update `remaining_gas -= op.get_gas_usage()`
    * update `remaining_space -= op.serialized_version.len()`
    * update `balance_cache[op.creator_addr] -= (op.fee + op.get_gas_coins())`
* deduce operation count in the header from the operation list length
* deduce the operation hash in the header by hashing the concatenation of the contained operation IDs, in the order they appear in the block
* produce the header and sign it
* store the block in storage
* notify `consensus` with the block ID for processing
* `consensus` will send it further to protocol for propagation if it was properly integrated in the graph

## Endorsement production

Endorsement production is slightly different from the one we have currently.
Endorsement production should happen in a separate thread compared to block creation.

### Timing


Endorsements that endorse slot `S=(period, thread)` will be produced at endorsement tick `genesis_timestamp + (t0*period) + ((thread*t0)/thread_count)) + (t0/2)`.
This is indeed a half-period lag in order to incentivize timely block and endorsement propagation.

Since double-staking is penalized and could happen on node reboot, a safety measure is to wait 1 period (`2*t0`) after the program starts before beginning endorsement production.

At every block creation slot, the block creation thread will read selector draws at that slot to check whether one of the staking keys was selected to produce the block.
If the address linked to a stored staking key was selected, launch the block production process below.


### Endorsement production

At each endorsement time tick encorsing slot `S`, we do the following:
* ask pos for endorsement draws, and list all slot indices for which our staking address
  * there can be multiple draws for different endorsement indices at the same slot with the same or different addresses 
* ask consensus to give the block ID of a block `B` of the blockclique at slot `S`
  * if there is none, do nothing for this tick
* for each endorsement `endo` that is supposed to be created by address `A` and endorse the block `B`:
  * create an endorsement endorsing `B.id` at slot `S`, signed with `A`
* send all created endorsements to `storage` and `protocol` for propagation

