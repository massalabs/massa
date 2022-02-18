# Read-only execution

# Rationale

When using Massa in a Web3 context for example, one should be able to perform read-only Smart Contract calls.

See: https://ethereum.stackexchange.com/questions/765/what-is-the-difference-between-a-transaction-and-a-call/770

# Massa implementation

## API

Add a "sc_readonly_call" API endpoint 

Parameters:
* max_gas: u64  // max gas allowed for the readonly run
* simulated_gas_price: Amount  // simulated gas price to expose to the smart contract context
* simulated_caller: Option<Address> // pretend this address is executing the SC, if none provided a random one will be used.
* bytecode: `Vec<u8>`  // bytecode to execute

Return value:
* executed_at: Slot  // slot at which the execution occurred
* result:
  * (optional) error: Error 
  * (optional) output_events: `Vec<SCOutputEvent>`  // output events generated during execution

 ## Operation

* when the sc_readonly_call is called, the bytecode's main() function will be called with the following execution context:
  * the execution will be done from the point of view of the latest slot at the current timestamp (see VM slot filler)
  * Clear and update the context.
  * set the call stack to simulated_caller_address
  * set max_gas to its chosen value
  * set gas_price to simulated_gas_price
  * TODO: block ? maybe just assume a miss
  * Note: do not apply changes to the ledger.

* during the call, everything happens as with a normal ExecuteSC call, but when the call finishes, its effects are rollbacked (like when a SC execution fails)