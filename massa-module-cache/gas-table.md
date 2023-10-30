# Overview

This document keeps track of where and how gas costs are handled for SC executions.

It contains:
* the gas table
* the list of functions that effectively pay the costs

# Gas table

| TYPE                     | PAYMENT                     |
|--------------------------|-----------------------------|
| **CallSC (OP)**          |                             |
| compilation              | NOT PAID                    |
| VM instantiation         | PAID IN MAX_GAS (UNIFIED CONSTANT)  |
| module instantiation     | PAID IN MAX_GAS             |
| execution                | PAID IN MAX_GAS             |
| **ExecuteSC (OP)**       |                             |
| compilation              | PAID IN MAX_GAS             |
| VM instantiation         | PAID IN MAX_GAS (UNIFIED CONSTANT)  |
| module instantiation     | PAID IN MAX_GAS             |
| execution                | PAID IN MAX_GAS             |
| **Call (ABI)**           |                             |
| compilation              | NOT PAID                    |
| VM instantiation         | PAID IN CALL GAS (UNIFIED CONSTANT) |
| module instantiation     | PAID IN CALL GAS            |
| execution                | PAID IN CALL GAS            |
| base gas cost of the ABI | PAID IN ABI GAS             |
| **LocalExecution (ABI)** |                             |
| compilation              | PAID IN CALL GAS            |
| VM instantiation         | PAID IN CALL GAS (UNIFIED CONSTANT) |
| module instantiation     | PAID IN CALL GAS            |
| execution                | PAID IN CALL GAS            |
| base gas cost of the ABI | PAID IN ABI GAS             |
| **CreateSC (ABI)**       |                             |
| compilation              | PAID IN ABI GAS             |
| **SetBytecode (ABI)**    |                             |
| compilation              | PAID IN ABI GAS             |

# Functions

### Singlepass compilation

1. Paid in `massa-module-cache` > `controller.rs` > `load_tmp_module`
2. Called in `massa-execution-worker` > `execution.rs` > `execute_executesc_op` & `execute_readonly_request`

### Cranelift compilation

1. Paid in `massa-execution-worker` > `context.rs` > `create_new_sc_address` & `set_bytecode`
2. Used in `massa-execution-worker` > `interface_impl.rs` > `create_module`, `raw_set_bytecode` & `raw_set_bytecode_for`
3. Called by `massa-sc-runtime` corresponding ABIs, respectively `assembly_script_create_sc`, `assembly_script_set_bytecode` & `assembly_script_set_bytecode_for`

### VM & Module instantiation

1. Threshold checked in `massa-module-cache` > `controller.rs` > `load_module` & `load_tmp_module`
2. Actual cost paid in `massa-sc-runtime`

### Execution

1. Paid in `massa-sc-runtime`
