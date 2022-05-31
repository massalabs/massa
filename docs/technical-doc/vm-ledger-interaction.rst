=====================
VM ledger interaction
=====================

Rationale
=========

This page describes how the VM interacts with the SCE ledger.

Summary
=======

VM execution happens in steps: there is one execution step at every slot.
During the execution of a slot S, background async tasks at S are executed first, then the block at slot S (if any) is executed.

The SCE ledger is a hashmap mapping an Address to a balance, optional bytecode, and a datastore hashmap mapping hashes to arbitrary bytes.

The VM maintains a single full copy of the SCE ledger at the output of the latest SCE-final slot,
as well as an active execution step history containing the changes caused to the ledger by every active slot.
The step history can be sequentially applied on top of the SCE-final ledger to get an SCE ledger entry at the output of any active slot.

During execution, the executed code can access the SCE ledger with read and limited write rights.

Design
======

SCE Ledger
----------

**Structure**

The SCE Ledger can become large (up to 1TB). For now, it is in RAM but this needs to be fixed.

The SCE Ledger is represented by the `SCELedger` structure which acts as a hashmap associating an `Address` to a `SCELedgerEntry`.

The `SCELedgerEntry` structure represents an entry in the ledger and has the following properties:
* `balance`: the SCE balance of the address
* `opt_module`: an optional executable module
* `data: HHashMap<Hash, Vec<u8>>`: a generic datastore associating a hash to bytes

**Ledger changes**

The `SCELedgerChanges` struct is a hashmap associating an `Address` to a `SCELedgerChange` and represents the entries of the SCE ledger that have changed. The exact change to each entry is described by the `SCELedgerChange` enum that can be:
* `Delete`: the entry was deleted
* `Set(SCELedgerEntry)`: a new entry was inserted or an existing entry was reset to a completely new value
* `Update(SCELedgerEntryUpdate)`: an existing entry was modified. The modifications are described by `SCELedgerEntryUpdate`

The `SCELedgerEntryUpdate` struct describes modifications to an SCE ledger entry and has the following fields:
* `update_balance: Option<Amount>`: optionally updates the balance of the entry to a new value
* `update_opt_module: Option<Option<Bytecode>>`: optionally updates the module of the entry to a new one (or to None)
* `update_data: HHashMap<Hash, Option<Vec<u8>>>`: a list of datastore entries that have been updated to a new value (or deleted if the hashmap value is None)


VM structure
------------

**Overview**

The VM is represented by the `VM` structure with the following properties:
* `step_history`: a history of active execution steps
* `execution_interface`: an interface for the interpreter
* `execution_context`: an execution context

Those fields are described in the next subsections.

**Step history**

The VM contains a `step_history` property which is a list of `StepHistoryItem` active steps that have been executed on top of the current final SCE ledger. Each `StepHistoryItem` represents the summarized consequences of a given active step and has the following properties:
* `slot`: the slot to which the step is associated
* `opt_block_id`: an optional block ID if a block is present at that slot, or None if there is a miss
* `ledger_changes`: a `SCELedgerChanges` object listing the SCE ledger changes caused by that step

The state of an entry of the SCE ledger at the output of a given active execution step can be retrieved by taking the corresponding entry in the final SCE ledger (available in the execution context, see below) and applying ledger changes from the `step_history` one after the other until the desired active step (included).

**Execution interface**

TODO

**Execution context**

The `execution_context` field of the `VM` sturct represents the context in which the current execution runs.
`ExecutionContext` has the following fields:

* `ledger_step` is a `SCELedgerStep` that represents the state of the SCE ledger up to the latest point of the execution of the latest active step
* `max_gas` is the max amount of gas for the execution
* `coins` is the amount of coins that have been transferred to the called SC in the context of the call
* `gas_price` is the price (in coins) per unit of gas for the execution
* `slot` is the slot of the execution step
* `opt_block_id` block id being executed (None if absent)
* `opt_block_creator_addr` address of the block producer (None if block absent)
* `call_stack`: call stack listing calling addresses. The latest one is rightmost and should be the address of the called SC when applicable

The `SCELedgerStep` struct allows accumulating changes caused by the step execution and reading the latest SCE ledger state during execution. Fields:
* `final_ledger_slot` a `FinalLedger` structure containing the current final `SCELedger` as well as the slot at the output of which the final ledger is attached
* `cumulative_history_changes` is a `SCELedgerChanges` obtained by accumulating the `ledger_changes` of all the previous elements of the VM's `step_history`
* `caused_changes` is a `SCELedgerChanges` representing all the changes that happened so far in the current execution

In order to transparently obtain a ledger entry at the current point of the execution, `SCELedgerStep` provides convenience methods that gather entries from the final ledger, apply the `cumulative_history_changes` and then the `caused_changes`. It also provides convenience methods for applying changes to `caused_changes`.

SCE interaction with the VM
---------------------------

**Active execution requests**

Whenever the SCE tells the VM to execute an active block or active miss at slot S (see the [VM block feed specification](vm-block-feed)), the corresponding execution step is executed by the VM and the state changes caused by the execution are compiled into a `StepHistoryItem` and added to the `step_history`.

The detailed algorithm is the following:
* get the execution context ready by resetting `ledger_step.caused_changes` and computing `ledger_step.cumulative_history_changes` based on the `step_history`
* TODO run async background tasks
* if there is a block B at slot S:
  * Note that the block would have been rejected before if the sum of the `max_gas` of its operations exceeded `config.max_block_gas`
  * for every `ExecuteSC` operation Op of the block B :
    * Note that Consensus has already debited `Op.max_gas*Op.gas_price+Op.coins` from Op's sender's CSS balance or rejected the block B if there wasn't enough balance to do so
    * prepare the context for execution:
      * make `context.ledger_step` credit Op's sender with `Op.coins` in the SCE ledger 
      * make `context.ledger_step` credit the producer of the block B with `Op.max_gas * Op.gas_price` in the SCE ledger
    * save a snapshot (named `ledger_changes_backup`) of the `context.ledger_step.caused_changes` that will be used to rollback the step's effects on the SCE ledger backt to this point in case bytecode execution fails. This is done because on bytecode execution failure (whether it fails completely or midway) we want to credit the block producer with fees (it's not their fault !) and Op's sender with `Op.coins` (otherwise those coins will be lost !) but revert all the effects of a bytecode execution that failed midway
    * parse and run (call `main()`) the bytecode of operation Op
      * in case of failure (e.g. invalid bytecode), revert `context.ledger_step.caused_changes = ledger_changes_backup`
* push back the SCE ledger changes caused by the slot `StepHistoryItem { step, block_id (optional), ledger_changes: context.ledger_step.caused_changes  }` into `step_history`


**Final execution requests**

Whenever the SCE tells the VM to execute a final block or final miss at slot S (see the [VM block feed specification](vm-block-feed)), the VM first checks if that step was already executed (it should match the first/oldest step in `step_history`).
If it matches (it should almost always), the step result is popped out of `step_history` and its `ledger_changes` are applied to the SCE final ledger. 

In the case where the step is not found at the front of `step_history`, it might mean that there was a deep blockclique change, or that there was nothing in `step_history` due to a recent bootstrap for example. In that case, `step_history` is cleared, the `Active execution requests` process described above is executed again, and its resulting history item is then applied to the final SCE ledger.

After this process, the SCE final ledger now represents the SCE ledger state at the output of slot S.



ABIs for interacting with the SCE ledger from inside running bytecode
=====================================================================

TODO detail how each one works


.. code-block::

    // gets the current execution context
    get_context() -> {
        // call stack
        call_stack: Vec<Address>
        // last item (stack top) is the current SC context, first item (stack bottom) is the initial caller 
        
        // number of coins transferred to the called address during the call
        transferred_coins: int
        
        // max gas
        max_gas: int
        
        // gas price
        gas_price: int
        
        // block ID in which the execution happens (only for ExecuteSC)
        block_id: Option<int> 
        
        // are we in a read-only execution context ?
        is_readonly: bool
        
        // remaining gas
        remaining_gas
    }

    // transfer coins from the current address (if any) towards another
    transfer_coins(recipient_address, amount) -> Result<()>

    // get the balance of an address
    get_balance(address) -> Result<int>

    // read the bytecode of an address
    get_bytecode(address) -> Result<Vec<u8>>

    // runs arbitrary bytecode in the current context by calling a function in it
    run_bytecode(bytecode, function_name, parameters, max_gas, gas_price, coins)

    // create a new ledger entry and initialize it with a balance and bytecode
    // returns the address of the entry
    create_sc(balance: int, bytecode) -> Result<Address>

    // delete the current address from the ledger, sending freed coins to a recipient
    self_destruct(recipient_addr)

    // calls a public method of a target SC in the context of the target SC
    call(target_addr, function_name, params, max_gas, gas_price, coins) -> Result< ... Return type ? ... >

    // gets data from an addresses' storage
    data_get(addr: Address, key: Hash) -> Result<Vec<u8>>

    // sets data in the current addresses' storage (insert if absent)
    data_set(key: Hash, value: Vec<u8>) -> Result<()>

    // delete data in the current addresses's storage
    data_remove(key: Hash) -> Result<bool>
