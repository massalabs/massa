# Object storage management

## The `massa-storage` crate and the `Storage` structure it provides

### General description

This crate provides the `Storage` structure which give access to a globally shared object store containing blocks, endorsements and operations.
It also allows locally claiming references to a list of shared objects for a particular instance of `Storage`.
When no instance of `Storage` owns a reference to a given object anymore, that object is automatically dropped from the globally shared object store. 

The `Storage` structure contains:
* shared references to global maps containing objects like blocks, endorsements, operations and indices allowing to retrieve them by various criteria
* shared reference counters to those objects
* a local (non-shared) list of object references owned by that particular instance of `Storage`

### Contructing and cloning `Storage`

An initial `Storage` object should be created in the program's entry point using:

```rust
let storage: Storage = Default::default();
```

**All subsequent instances of `Storage` used throughout the program should always be clones of that instance.**

There are two ways of cloning an instance of `Storage`:
* `storage.clone()` will clone an existing instance and claim all the references to objects that the initial instance held
* `storage.clone_without_refs()` will clone an existing instance but the resulting instance will not hold any references to objects

### Adding an element to the global shared object store

Here we take the example of adding a set of operations to the global shared object store.
Note that the same applies to blocks and endorsements.

```rust
let operations: Vec<WrappedOperation> = XXXX;
storage.store_operations(operations);
// Here, all `operations` were stored inside the global shared object store and `storage` has acquired references to all of them. Objects that were already stored beforehand are not overwritten, but a reference is simply acquired. 
```

### Claiming a reference to an object that is already in the globally shared store

Here we take the example of claiming a reference to a set of operations that is already in the global shared object store.
Note that the same applies to blocks and endorsements.

```rust
let operation_ids: Set<OperationId> = XXXX;
let claimed: Set<OperationId> = storage.claim_operation_refs(&operation_ids);
// Here, `claimed` contains the set of operation IDs among `operation_ids` for which references were successfully claimed by the `storage` instance. Elements of `operation_ids` that were not found in the global shared object store are absent from `claimed`.
```

### Dropping references to objects

When a `Storage` instance is dropped, all its references are automatically dropped.

To drop a specific reference to an operation for example, use:
```rust
let operation_ids: Set<OperationId> = XXXX;
storage.drop_block_refs(&operation_ids);
// Here, `storage` does not own any references to operations listed `operation_ids` anymore. Operations that were not owned beforehand are ignored.
```
When not a single `Storage` instance references a given object anymore, that object is removed from the global shared object store.

### Listing locally owned objects

To get a read-only reference to the set of locally owned operations for example, simply use:
```rust
let local_op_refs: &Set<OperationId> = storage.get_op_refs();
```

### Merging, splitting off

* `storage.extend(other)` consumes `other` and adds its locally owned object references to `storage`
* `let new_storage = storage.split_off(&block_id_set, &operation_id_set, &endorsement_id_set);` efficiently transfers ownership of sets of local object references from `storage` to a new `new_storage` instance. `storage` loses the transferred references, and `new_storage` acquires them.

### Accessing objects in the global shared object store

To get a reference to the global shared operation store for example, use
```rust
let global_op_store = storage.read_operations();  // this effectively acquires a read-lock on the global shared operations store, and should be released as quickly as possible
```

From there, it is possible to get a reference to a stored operation using its ID:
```rust
let op_id: OperationId = XXXX;
let op_ref: Option<&WrappedOperation> = global_op_store.get(&op_id);  // returns None if the operation was not found
```

Multiple indices are available for efficiently listing and accessing objects by criteria:
```rust
// Example: listing all operations created by an address
let creator_address: Address = XXXX;
let ops_created: Option<&Set<OperationId>> = global_op_store.get_operations_created_by(&creator_address);  // returns None if the index is empty
// Note that a reference to a Set is returned to avoid copies of large indices
```

Here are the available query criteria:
* In `storage.read_operations()`:
  * `get(id: &OperationId) -> Option<&WrappedOperation>` returns a reference to the stored operation
  * `get_operations_created_by(addr: &Address) -> Option<&Set<OperationId>>` returns a reference to the set of operation IDs created by a given address
* In `storage.read_endorsements()`:
  * `get(id: &EndorsementId) -> Option<&WrappedEndorsement>` returns a reference to the stored endorsement
  * `get_endorsements_created_by(addr: &Address) -> Option<&Set<EndorsementId>>` returns a reference to the set of endorsement IDs created by a given address
* In `storage.read_blocks()`:
  * `get(id: &BlockId) -> Option<&WrappedBlock>` returns a reference to the stored block
  * `get_blocks_created_by(addr: &Address) -> Option<&Set<BlockId>>` returns a reference to the set of block IDs created by a given address
  * `get_blocks_by_slot(slot: &Slot) -> Option<&Set<BlockId>>` returns a reference to the set of block IDs that belong to a given slot
  * `get_blocks_by_operation(id: &OperationId) -> Option<&Set<BlockId>>` returns a reference to the set of block IDs that contain a given operation
  * `get_blocks_by_endorsement(id: &EndorsementId) -> Option<&Set<BlockId>>` returns a reference to the set of block IDs that contain a given endorsement

## Storage management in Pool

Pools only reference operations and endorsements.

The operation pool (resp. endorsement pool) has its own instance of `Storage` that owns references to all the operations (resp. endorsements) currently in the pool.

When sending a set of operations to the operation pool, simply use `PoolController::add_operations(storage)`. All the the operations that `storage: Storage` locally owns references to will be added to the operation pool.

When sending a set of endorsements to the operation pool, simply use `PoolController::add_endorsements(storage)`. All the the endorsements that `storage: Storage` locally owns references to will be added to the endorsement pool.

When an object is pruned from a pool, its reference in that pool's storage instance is dropped.

When asking pool for a set of endorsements to create a block, use `PoolController::get_block_endorsements(..) -> (Vec<Option<EndorsementId>>, Storage)` where the returned `Vec` is used to keep the order of the endorsements for which references are owned in the returned `Storage` instance.

When asking pool for a set of operations to create a block, use `PoolController::get_block_operations(..) -> (Vec<OperationId>, Storage)` where the returned `Vec` is used to keep the order of the operations for which references are owned in the returned `Storage` instance.

## Storage management in Factory

The endorsement factory simply produces a batch of endorsements whenever one or more of its addresses are selected to produce endorsements at the current slot. That batch is added to a clean instance of `storage: Storage` which is then sent to `Protocol` for propagation using `ProtocolCommandSender::propagate_endorsements(storage.clone())` and to `Pool` using `PoolController::add_endorsements(storage)`.

The block factory works according the following steps whenever an owned address is selected to create a block at the current slot:
* factory first prepares an instance of `block_storage: Storage` that owns no references.
* factory then extends `block_storage` with the endorsements obtained from `PoolController::get_block_endorsements(..) -> (Vec<Option<EndorsementId>>, Storage)`.
* factory then extends `block_storage` with the operations obtained from `PoolController::get_block_operations(..) -> (Vec<OperationId>, Storage)`.
* factory then assembles the resulting block and stores it in `block_storage` as well.
* factory then sends the block to consensus through `ConsensusCommandSender::send_block(block_id, slot, block_storage)`.

## Storage management in the API

When sending a batch of operations from the API, they need to be deserialized and added to an instance of `storage:Storage` that is then sent to `Pool` using `PoolController::add_operations(storage.clone())` and to `Protocol` using `ProtocolCommandSender::propagate_operations(storage)` for propagation.

When the `get_operations`, `get_endorsements`, `get_block` or `get_addresses` is called the API will look into the his reference of the storage to find all the operations, endorsements, blocks or all informations related to an address.

## Storage management in Consensus/Graph

Consensus and Graph only manage blocks.

Each block managed by `Consensus/Graph` ia accompanied by its specific instance of `Storage` that owns references to:
* the block itself
* the endorsements contained in the block
* the operations contained in the block
* the parents of the block (when available)
That way, dropping the block's `Storage` instance allows dropping references to all its dependencies at the same time.

Full blocks are sent to `Consensus` through `ConsensusCommandSender::send_block(block_id, slot, block_storage)`. The provided `block_storage: Storage` needs to own references to the block itself, along with its operations and endorsements, but not necessarily its parents as they might not be available in the global shared block store yet. Note that providing `block_id` allows making sure that we are extracting the target block from the provided `block_storage` and not any of its dependency blocks.

When `Consensus` manages to add the block to the graph, it adds the references to the block's parents to the block's `Storage` instance, and calls `ProtocolCommandSender::integrated_block(block.id, block.storage.clone())` for `Protocol` to propagate the block.

When bootstrapping a client, we need to send every block's dependencies together with the block. This is why `ExportActiveBlock` contains a `pub operations: Vec<WrappedOperation>` field to carry the operations on serialization.

When bootstrapping from a server, and downloading an `ExportActiveBlock`, the deserialized objects (block, operations, endorsements) all need to be added to a clean `Storage` instance specific to that block.
When full graph is reconstructed, a pass is needed on every block to add its parents to its associated storage (if they are available).


## Storage management in Protocol

TODO @adrien-zinger @gterzian can you write this part ?
