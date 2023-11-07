# Overview

This document keeps track of where and how gas costs are handled for SC executions.

It contains:
* the gas table
* the list of functions that effectively pay the costs

# Gas table

| TYPE                     | PAYMENT          |
| ------------------------ | ---------------- |
| **CallSC (OP)**          |                  |
| compilation              | NOT PAID         |
| VM instantiation         | PAID IN MAX_GAS  |
| module instantiation     | PAID IN MAX_GAS  |
| execution                | PAID IN MAX_GAS  |
| **ExecuteSC (OP)**       |                  |
| compilation              | PAID IN OP GAS   |
| VM instantiation         | PAID IN MAX_GAS  |
| module instantiation     | PAID IN MAX_GAS  |
| execution                | PAID IN MAX_GAS  |
| **Call (ABI)**           |                  |
| compilation              | NOT PAID         |
| VM instantiation         | PAID IN CALL GAS |
| module instantiation     | PAID IN CALL GAS |
| execution                | PAID IN CALL GAS |
| base gas cost of the ABI | PAID IN ABI GAS  |
| **LocalExecution (ABI)** |                  |
| compilation              | PAID IN ABI GAS  |
| VM instantiation         | PAID IN CALL GAS |
| module instantiation     | PAID IN CALL GAS |
| execution                | PAID IN CALL GAS |
| base gas cost of the ABI | PAID IN ABI GAS  |
| **CreateSC (ABI)**       |                  |
| compilation              | PAID IN ABI GAS  |
| **SetBytecode (ABI)**    |                  |
| compilation              | PAID IN ABI GAS  |

# Functions

### Singlepass compilation

1. Paid for ExecuteSC operations as OP cost in `massa-execution-worker` > `execution.rs` > `execute_operation` by `get_gas_usage`
2. Paid for ReadOnly requests in `massa-execution-worker` > `execution.rs` > `execute_readonly_request`
3. Paid in `massa-sc-runtime` ABI cost by `assembly_script_local_execution`. This ABI gives rise to Singlepass compilations and must have according costs to pay for it.

### Cranelift compilation

Paid in `massa-sc-runtime` ABI costs by `assembly_script_create_sc`, `assembly_script_set_bytecode` & `assembly_script_set_bytecode_for`. These ABIs give rise to Cranelift compilations and must have according costs to pay for it.

### VM & Module instantiation

1. Threshold checked in `massa-module-cache` > `controller.rs` > `load_module` & `load_tmp_module`
2. `load_module` is used in every function calling SCs stored on the chain
3. `load_tmp_module` is used in every function calling arbitrary code
4. Actual cost paid in `massa-sc-runtime`

### Execution

Paid in `massa-sc-runtime`.
