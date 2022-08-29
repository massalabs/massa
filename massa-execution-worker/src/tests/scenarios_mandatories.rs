// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::start_execution_worker;
use massa_async_pool::AsyncPoolConfig;
use massa_execution_exports::{
    ExecutionConfig, ExecutionError, ReadOnlyExecutionRequest, ReadOnlyExecutionTarget,
};
use massa_final_state::{FinalState, FinalStateConfig};
use massa_hash::Hash;
use massa_ledger_exports::{LedgerConfig, LedgerError};
use massa_ledger_worker::FinalLedger;
use massa_models::config::{
    ASYNC_POOL_PART_SIZE_MESSAGE_BYTES, MAX_ASYNC_POOL_LENGTH, MAX_DATA_ASYNC_MESSAGE,
};
use massa_models::{address::Address, amount::Amount, slot::Slot};
use massa_models::{
    api::EventFilter,
    block::{Block, BlockHeader, BlockHeaderSerializer, BlockId, BlockSerializer, WrappedBlock},
    config::THREAD_COUNT,
    operation::{Operation, OperationSerializer, OperationType, WrappedOperation},
    wrapped::WrappedContent,
};
use massa_pos_exports::SelectorConfig;
use massa_pos_worker::start_selector_worker;
use massa_signature::KeyPair;
use massa_storage::Storage;
use parking_lot::RwLock;
use serial_test::serial;
use std::{
    cmp::Reverse,
    collections::{BTreeMap, HashMap},
    str::FromStr,
    sync::Arc,
    time::Duration,
};
use tempfile::{NamedTempFile, TempDir};

use super::mock::get_initial_rolls;

/// Same as `get_random_address()` and return `keypair` associated
/// to the address.
pub fn get_random_address_full() -> (Address, KeyPair) {
    let keypair = KeyPair::generate();
    (Address::from_public_key(&keypair.get_public_key()), keypair)
}

/// Get a randomized address
pub fn get_random_address() -> Address {
    get_random_address_full().0
}

fn get_sample_state() -> Result<(Arc<RwLock<FinalState>>, NamedTempFile, TempDir), LedgerError> {
    let mut initial: BTreeMap<Address, Amount> = Default::default();
    initial.insert(get_random_address(), Amount::from_str("129").unwrap());
    initial.insert(get_random_address(), Amount::from_str("878").unwrap());
    let (ledger_config, tempfile, tempdir) = LedgerConfig::sample(&initial);
    let file = get_initial_rolls();
    let ledger = FinalLedger::new(ledger_config.clone()).expect("could not init final ledger");
    let async_pool_config = AsyncPoolConfig {
        max_length: MAX_ASYNC_POOL_LENGTH,
        part_size_message_bytes: ASYNC_POOL_PART_SIZE_MESSAGE_BYTES,
        max_data_async_message: MAX_DATA_ASYNC_MESSAGE,
        thread_count: THREAD_COUNT,
    };
    let cfg = FinalStateConfig {
        ledger_config,
        async_pool_config,
        final_history_length: 128,
        thread_count: THREAD_COUNT,
        initial_rolls_path: file.path().to_path_buf(),
        initial_seed_string: "".to_string(),
        periods_per_cycle: 10,
    };
    let (_, selector_controller) = start_selector_worker(SelectorConfig::default())
        .expect("could not start selector controller");
    Ok((
        Arc::new(RwLock::new(
            FinalState::new(cfg, Box::new(ledger), selector_controller).unwrap(),
        )),
        tempfile,
        tempdir,
    ))
}

#[test]
#[serial]
fn test_execution_shutdown() {
    let (sample_state, _keep_file, _keep_dir) = get_sample_state().unwrap();
    let (_selector_manager, selector_controller) =
        start_selector_worker(SelectorConfig::default()).unwrap();
    let (mut manager, _) = start_execution_worker(
        ExecutionConfig::default(),
        sample_state,
        selector_controller,
    );
    manager.stop();
}

#[test]
#[serial]
fn test_sending_command() {
    let (sample_state, _keep_file, _keep_dir) = get_sample_state().unwrap();
    let (_selector_manager, selector_controller) =
        start_selector_worker(SelectorConfig::default()).unwrap();
    let (mut manager, controller) = start_execution_worker(
        ExecutionConfig::default(),
        sample_state,
        selector_controller,
    );
    controller.update_blockclique_status(Default::default(), Default::default());
    manager.stop();
}

#[test]
#[serial]
fn test_sending_read_only_execution_command() {
    let (sample_state, _keep_file, _keep_dir) = get_sample_state().unwrap();
    let (_selector_manager, selector_controller) =
        start_selector_worker(SelectorConfig::default()).unwrap();
    let (mut manager, controller) = start_execution_worker(
        ExecutionConfig::default(),
        sample_state,
        selector_controller,
    );
    controller
        .execute_readonly_request(ReadOnlyExecutionRequest {
            max_gas: 1_000_000,
            simulated_gas_price: Amount::from_mantissa_scale(1_000_000, 0),
            call_stack: vec![],
            target: ReadOnlyExecutionTarget::BytecodeExecution(
                include_bytes!("./wasm/event_test.wasm").to_vec(),
            ),
        })
        .unwrap();
    manager.stop();
}

/// Test the gas usage in nested calls using call SC operation
///
/// Create a smart contract and send it in the blockclique.
/// This smart contract have his sources in the sources folder.
/// It calls the test function that have a sub-call to the receive function and send it to the blockclique.
/// We are checking that the gas is going down through the execution even in sub-calls.
///
/// This test can fail if the gas is going up in the execution
#[test]
#[serial]
fn test_nested_call_gas_usage() {
    // setup the period duration
    let exec_cfg = ExecutionConfig {
        t0: 1000.into(),
        ..ExecutionConfig::default()
    };
    // get a sample final state
    let (sample_state, _keep_file, _keep_dir) = get_sample_state().unwrap();
    // init the storage
    let mut storage = Storage::default();
    // start the execution worker
    let (_selector_manager, selector_controller) =
        start_selector_worker(SelectorConfig::default()).unwrap();
    let (mut manager, controller) =
        start_execution_worker(exec_cfg, sample_state, selector_controller);
    // get random keypair
    let (_, keypair) = get_random_address_full();
    // load bytecode you can check the source code of the
    // following wasm file in massa-sc-examples
    let bytecode = include_bytes!("./wasm/nested_call.wasm");
    // create the block containing the smart contract execution operation
    let operation = create_execute_sc_operation(&keypair, bytecode).unwrap();
    storage.store_operations(vec![operation.clone()]);
    let block = create_block(vec![operation], Slot::new(1, 0)).unwrap();
    // store the block in storage
    storage.store_block(block.clone());

    // set our block as a final block so the message is sent
    let mut finalized_blocks: HashMap<Slot, (BlockId, Storage)> = Default::default();
    finalized_blocks.insert(
        block.content.header.content.slot,
        (block.id, storage.clone()),
    );
    controller.update_blockclique_status(finalized_blocks.clone(), Default::default());

    // sleep for 300ms to reach the message execution period
    std::thread::sleep(Duration::from_millis(10));
    // retrieve events emitted by smart contracts
    let events = controller.get_filtered_sc_output_event(EventFilter {
        start: Some(Slot::new(0, 1)),
        end: Some(Slot::new(20, 1)),
        ..Default::default()
    });
    // match the events
    assert!(!events.is_empty(), "One event was expected");
    let address = events[0].clone().data;
    // Call the function test of the smart contract
    let operation = create_call_sc_operation(
        &keypair,
        10000000,
        Amount::from_str("0").unwrap(),
        Address::from_str(&address).unwrap(),
        String::from("test"),
        address,
    )
    .unwrap();
    // Init new storage for this block
    let mut storage = Storage::default();
    storage.store_operations(vec![operation.clone()]);
    let block = create_block(vec![operation], Slot::new(1, 1)).unwrap();
    // store the block in storage
    storage.store_block(block.clone());
    // set our block as a final block so the message is sent
    let mut finalized_blocks: HashMap<Slot, (BlockId, Storage)> = Default::default();
    finalized_blocks.insert(
        block.content.header.content.slot,
        (block.id, storage.clone()),
    );
    controller.update_blockclique_status(finalized_blocks, Default::default());
    std::thread::sleep(Duration::from_millis(300));
    // Get the events that give us the gas usage (refer to source in ts) without fetching the first slot because it emit a event with an address.
    let events = controller.get_filtered_sc_output_event(EventFilter {
        start: Some(Slot::new(1, 1)),
        ..Default::default()
    });
    // Check that we always subtract gas through the execution (even in sub calls)
    assert!(
        events.is_sorted_by_key(|event| Reverse(event.data.parse::<u64>().unwrap())),
        "Gas is not going down through the execution."
    );
    // stop the execution controller
    manager.stop();
}

/// # Context
///
/// Functional test for asynchronous messages sending and handling
///
/// 1. a block is created containing an `execute_sc` operation
/// 2. this operation executes the `send_message` of the smart contract
/// 3. `send_message` stores the `receive_message` of the smart contract on the block
/// 4. `receive_message` contains the message handler function
/// 5. `send_message` sends a message to the `receive_message` address
/// 6. we set the created block as finalized so the message is actually sent
/// 7. we execute the following slots for 300 milliseconds to reach the message execution period
/// 8. once the execution period is over we stop the execution controller
/// 9. we retrieve the events emitted by smart contract, filtered by the message execution period
/// 10. `receive_message` handler function should have emitted an event
/// 11. we check if they are events
/// 12. if they are some, we verify that the data has the correct value
///
#[test]
#[serial]
fn send_and_receive_async_message() {
    // setup the period duration and the maximum gas for
    // asynchronous messages execution
    let exec_cfg = ExecutionConfig {
        t0: 1000.into(),
        max_async_gas: 100_000,
        ..ExecutionConfig::default()
    };
    // get a sample final state
    let (sample_state, _keep_file, _keep_dir) = get_sample_state().unwrap();

    // init the storage
    let mut storage = Storage::default();
    // start the execution worker
    let (_selector_manager, selector_controller) =
        start_selector_worker(SelectorConfig::default()).unwrap();
    let (mut manager, controller) =
        start_execution_worker(exec_cfg, sample_state, selector_controller);
    // keypair associated to thread 0
    let keypair = KeyPair::from_str("S1JJeHiZv1C1zZN5GLFcbz6EXYiccmUPLkYuDFA3kayjxP39kFQ").unwrap();
    // load send_message bytecode you can check the source code of the
    // following wasm file in massa-sc-examples
    let bytecode = include_bytes!("./wasm/send_message.wasm");
    // create the block contaning the smart contract execution operation

    let operation = create_execute_sc_operation(&keypair, bytecode).unwrap();
    storage.store_operations(vec![operation.clone()]);
    let block = create_block(vec![operation], Slot::new(1, 0)).unwrap();
    // store the block in storage
    storage.store_block(block.clone());

    // set our block as a final block so the message is sent
    let mut finalized_blocks: HashMap<Slot, (BlockId, Storage)> = Default::default();
    finalized_blocks.insert(
        block.content.header.content.slot,
        (block.id, storage.clone()),
    );
    controller.update_blockclique_status(finalized_blocks, Default::default());

    // sleep for 300ms to reach the message execution period
    std::thread::sleep(Duration::from_millis(300));

    // retrieve events emitted by smart contracts
    let events = controller.get_filtered_sc_output_event(EventFilter {
        start: Some(Slot::new(1, 1)),
        end: Some(Slot::new(20, 1)),
        ..Default::default()
    });
    // match the events
    assert!(!events.is_empty(), "One event was expected");
    assert_eq!(events[0].data, "message received: hello my good friend!");
    // stop the execution controller
    manager.stop();
}

#[test]
#[serial]
pub fn send_and_receive_transaction() {
    // setup the period duration and the maximum gas for
    let exec_cfg = ExecutionConfig {
        t0: 1000.into(),
        ..ExecutionConfig::default()
    };
    // get a sample final state
    let (sample_state, _keep_file, _keep_dir) = get_sample_state().unwrap();

    // init the storage
    let mut storage = Storage::default();
    // start the execution worker
    let (_selector_manager, selector_controller) =
        start_selector_worker(SelectorConfig::default()).unwrap();
    let (mut manager, controller) =
        start_execution_worker(exec_cfg, sample_state, selector_controller);
    // generate the addresses
    let (_addr, sender_keypair) = get_random_address_full();
    let (recipient_address, _keypair) = get_random_address_full();
    // create the operation
    let operation = Operation::new_wrapped(
        Operation {
            fee: Amount::zero(),
            expire_period: 10,
            op: OperationType::Transaction {
                recipient_address,
                amount: Amount::from_raw(42),
            },
        },
        OperationSerializer::new(),
        &sender_keypair,
    )
    .unwrap();
    // create the block contaning the transaction operation
    storage.store_operations(vec![operation.clone()]);
    let block = create_block(vec![operation], Slot::new(1, 0)).unwrap();
    // store the block in storage
    storage.store_block(block.clone());
    // set our block as a final block so the transaction is processed
    let mut finalized_blocks: HashMap<Slot, (BlockId, Storage)> = Default::default();
    finalized_blocks.insert(
        block.content.header.content.slot,
        (block.id, storage.clone()),
    );
    controller.update_blockclique_status(finalized_blocks, Default::default());
    // TODO: assert here

    // stop the execution controller
    manager.stop();
}

#[test]
#[serial]
fn generate_events() {
    // Compile the `./wasm_tests` and generate a block with `event_test.wasm`
    // as data. Then we check if we get an event as expected.
    let exec_cfg = ExecutionConfig {
        t0: 1000.into(),
        ..ExecutionConfig::default()
    };
    let mut storage: Storage = Default::default();
    let (sample_state, _keep_file, _keep_dir) = get_sample_state().unwrap();
    let (_selector_manager, selector_controller) =
        start_selector_worker(SelectorConfig::default()).unwrap();
    let (mut manager, controller) =
        start_execution_worker(exec_cfg, sample_state, selector_controller);

    let (sender_address, keypair) = get_random_address_full();
    let event_test_data = include_bytes!("./wasm/event_test.wasm");
    let operation = create_execute_sc_operation(&keypair, event_test_data).unwrap();
    storage.store_operations(vec![operation.clone()]);
    let block = create_block(vec![operation], Slot::new(1, 0)).unwrap();
    let slot = block.content.header.content.slot;

    storage.store_block(block.clone());

    let finalized_blocks: HashMap<Slot, (BlockId, Storage)> = Default::default();
    let mut blockclique: HashMap<Slot, (BlockId, Storage)> = Default::default();

    blockclique.insert(slot, (block.id, storage.clone()));

    controller.update_blockclique_status(finalized_blocks, blockclique);

    std::thread::sleep(Duration::from_millis(1000));
    let events = controller.get_filtered_sc_output_event(EventFilter {
        start: Some(slot),
        emitter_address: Some(sender_address),
        ..Default::default()
    });
    assert!(!events.is_empty(), "At least one event was expected");
    manager.stop();
}

/// Create an operation for the given sender with `data` as bytecode.
/// Return a result that should be unwrapped in the root `#[test]` routine.
fn create_execute_sc_operation(
    sender_keypair: &KeyPair,
    data: &[u8],
) -> Result<WrappedOperation, ExecutionError> {
    let op = OperationType::ExecuteSC {
        data: data.to_vec(),
        max_gas: 100_000,
        coins: Amount::from_raw(200_000),
        gas_price: Amount::from_mantissa_scale(1, 0),
    };
    let op = Operation::new_wrapped(
        Operation {
            fee: Amount::zero(),
            expire_period: 10,
            op,
        },
        OperationSerializer::new(),
        &sender_keypair,
    )?;
    Ok(op)
}

/// Create an operation for the given sender with `data` as bytecode.
/// Return a result that should be unwrapped in the root `#[test]` routine.
fn create_call_sc_operation(
    sender_keypair: &KeyPair,
    max_gas: u64,
    gas_price: Amount,
    target_addr: Address,
    target_func: String,
    param: String,
) -> Result<WrappedOperation, ExecutionError> {
    let op = OperationType::CallSC {
        max_gas,
        target_addr,
        parallel_coins: Amount::from_str("0").unwrap(),
        sequential_coins: Amount::from_str("0").unwrap(),
        gas_price,
        target_func,
        param,
    };
    let op = Operation::new_wrapped(
        Operation {
            fee: Amount::zero(),
            expire_period: 10,
            op,
        },
        OperationSerializer::new(),
        sender_keypair,
    )?;
    Ok(op)
}

/// Create an almost empty block with a vector `operations` and a random
/// creator.
///
/// Return a result that should be unwrapped in the root `#[test]` routine.
fn create_block(
    operations: Vec<WrappedOperation>,
    slot: Slot,
) -> Result<WrappedBlock, ExecutionError> {
    let creator_keypair = KeyPair::generate();

    let operation_merkle_root = Hash::compute_from(
        &operations.iter().fold(Vec::new(), |acc, v| {
            [acc, v.serialized_data.clone()].concat()
        })[..],
    );

    let header = BlockHeader::new_wrapped(
        BlockHeader {
            slot,
            parents: vec![],
            operation_merkle_root,
            endorsements: vec![],
        },
        BlockHeaderSerializer::new(),
        &creator_keypair,
    )?;

    Ok(Block::new_wrapped(
        Block {
            header,
            operations: operations.into_iter().map(|op| op.id).collect(),
        },
        BlockSerializer::new(),
        &creator_keypair,
    )?)
}
