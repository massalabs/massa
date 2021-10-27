# Smart contract execution

## How are smart contracts registered with the system?

A smart contract can be created by way of an operation, and when the block including that operation is deemed ready for execution, the contract will be sent to the execution system to be registered. 

```rust
/// Operation to execute a smart-contract.
ExecuteSC {
    data: Vec<u8>,
    max_gas: u32,
    gas_price: Amount
}
```

An executing smart contract can also register new contracts directly from within the execution system, by issuing `ExecuteSC` directly.

## How are smart contracts executed?

Contracts are executed on a per-block basis, where `consensus` will notify `execution` of the block being ready for execution. `execution` will then execute all contracts included in the block, in their order of inclusion.

when `execution` receives a `BlockCliqueChanged` message:
- reset the SC candidate state to the Final state
- sort all blockclique blocks by slot (increasing order)
- for each block B in that order, run the machine sequentially:
    - for each operation in B (in the order in which they appear):
    - execute the operation(see below for details)
    
When receiving a `BlocksFinal` message:
- consider a consensus-final block as SC-final if:
    - Its immediate predecessor in terms of slots is SC-final
    - if the immediate predecessor is missing, the block is only considered SC-final if there is a consensus-final block in the thread of the missing block, at a later period

When a block becomes SC-final:
- if the block causes a TransferToConsensus, send a ExecuteSC message to consensus with:
    - target_addr = the SC emitter of the TransferToConsensus
    - origin_slot = the slot of the SC-final block causing the TransferToConsensus
    - amount = amount to transfer from SC to consensus
    
When executing an operation:
- If the operation is `TransferToSC`, credit the spending address with massa coins in the SCLedger
- if the operation is `ExecuteSC`, atomically execute or rollback in case of failure.

## Communication between consensus and execution

- Whenever the blockclique changes, send the new blockclique to SC with a `BlockCliqueChanged(clique)` message.
- Whenever a block becomes final, notify SC with a `BlocksFinal(Block, Slot)` mmessage.

## Structure of the execution component

One thread to handle incoming messages from `consensus`, manage the queue of blocks to execute. 

Another thread to continuously execute blocks from the queue.

## Statefulness of the execution system

The execution system:
- Holds a separate ledger, the `SCLedger`, that matches addresses to a balance, storage area, and program area.
- Holds a `FinalState` and a `CandidateState`.

## Communication between execution and the rest of the system

A smart-contract will be able to call into Massa-specific API's, providing by the "Massa runtime". We could use "host functions" in wasmer to provide these API's to a running contract. See https://gitlab.com/massalabs/massa/-/issues/360

Available "syscalls":

- `Call(SC_addr, function_name, parameters)`: synchronously calls a function in the same, or another smart contract
- `TransferToConsensus(amount)`: transfers massa coins from SC to consensus. On the SC side, this just burns the coins.
- `WriteDatabase(key, value)`: writes in the current database
- `ReadDatabase(SC_addr, key, value)`: reads a value from an arbitrary database
- `GetContext -> Context`: returns the call context (block, stack, etc...)
- `NewSC(balance, data, program) -> SC_addr` : create a new smart contract with initial balance, data and program. Returns its automatically generated address on success.

## Metering:

- If the total gas usage goes above the max_gas of the operation, cancel the execution of the operation
- If gas usage for a block goes above max_block_gas, cancel the execution of the remainder of the block

## Compilation and storage of modules

I assume contracts will propagate over the network as bytes, and that when those are received by a node we'll have to compile those into a wasm module. We could store the compiled modules in `execution`, so that they are ready to be executed when `consensus` notifies their finality. If contracts are only received as operations in a block, then they would probably be compiled just when `consensus` notifies their finality.


## Example smart-contract

```python
# main code sc.py

import massa

@export_smart_contract_function
def resolve_domain(name):
    db = massa.db.open(
        massa.context.sc_addr  # address of the current smart contract
    )
    # here, db is available for read AND write because we opened the db of the current SC,
    # otherwise it would be read-only
    resolved_addr = db.get(massa.hash(name))
    if resolved_addr is None:
        return None
    return resolved_addr
    

@export_smart_contract_function
def register_domain(name, address):
    if resolve_domain(name) is not None:
        raise ValueError("domain already registered") 
    db = massa.db.open(massa.context.sc_addr)
    db.insert(massa.hash(name), address)

""" how to deploy:

compile_python_sc sc.py
this tool will:
    * compile the python into PYC bytecode
    * output a WASM program (sc.wasm) that looks like this:
      call_remote_sc [address of the python bytecode interpreter smart contract] [PYC bytecode]

then, we can call using the client:
    register_new_smart_contract sc.wasm [wallet_address] [max_gas] [gas_price] [inclusion_fee] [initial_coins]
    >>> output = error, or smart contract address on success
this command will:
    * turn a wasm smart contract into an ExecuteSC operation where the payload bytes are a wasm program that looks like this:
      register_new_smart_contract [content of sc.wasm] [initial_coins]
    * send that operation



When we want to use the resolver in another program:    
    
"""

import massa
import my_resolver

resolved_addr = my_resolver.resolve("test.com")
# this effectively does the following (hidden in the my_resolver include):
# sc = massa.sc.open(RESOLVER_SC_ADDRESS)
# return sc.exports.resolve_domain(domain_string)
```
