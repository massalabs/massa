// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::start_execution_worker;
use crate::tests::mock::{create_block, get_random_address_full, get_sample_state};
use massa_execution_exports::{
    ExecutionConfig, ExecutionError, ReadOnlyExecutionRequest, ReadOnlyExecutionTarget,
};
use massa_models::address::ADDRESS_SIZE_BYTES;
use massa_models::config::{LEDGER_ENTRY_BASE_SIZE, LEDGER_ENTRY_DATASTORE_BASE_SIZE};
use massa_models::prehash::PreHashMap;
use massa_models::{address::Address, amount::Amount, slot::Slot};
use massa_models::{
    api::EventFilter,
    block::BlockId,
    datastore::Datastore,
    operation::{Operation, OperationSerializer, OperationType, WrappedOperation},
    wrapped::WrappedContent,
};
use massa_signature::KeyPair;
use massa_storage::Storage;
use serial_test::serial;
use std::{
    cmp::Reverse, collections::BTreeMap, collections::HashMap, str::FromStr, time::Duration,
};

#[test]
#[serial]
fn test_execution_shutdown() {
    let (sample_state, _keep_file, _keep_dir) = get_sample_state().unwrap();
    let (mut manager, _controller) = start_execution_worker(
        ExecutionConfig::default(),
        sample_state.clone(),
        sample_state.read().pos_state.selector.clone(),
    );
    manager.stop();
}

#[test]
#[serial]
fn test_sending_command() {
    let (sample_state, _keep_file, _keep_dir) = get_sample_state().unwrap();
    let (mut manager, controller) = start_execution_worker(
        ExecutionConfig::default(),
        sample_state.clone(),
        sample_state.read().pos_state.selector.clone(),
    );
    controller.update_blockclique_status(Default::default(), Default::default());
    manager.stop();
}

#[test]
#[serial]
fn test_sending_read_only_execution_command() {
    let (sample_state, _keep_file, _keep_dir) = get_sample_state().unwrap();
    let (mut manager, controller) = start_execution_worker(
        ExecutionConfig::default(),
        sample_state.clone(),
        sample_state.read().pos_state.selector.clone(),
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
        t0: 100.into(),
        ..ExecutionConfig::default()
    };
    // get a sample final state
    let (sample_state, _keep_file, _keep_dir) = get_sample_state().unwrap();
    // init the storage
    let mut storage = Storage::create_root();
    // start the execution worker
    let (mut manager, controller) = start_execution_worker(
        exec_cfg.clone(),
        sample_state.clone(),
        sample_state.read().pos_state.selector.clone(),
    );
    // get random keypair
    let keypair = KeyPair::from_str("S1JJeHiZv1C1zZN5GLFcbz6EXYiccmUPLkYuDFA3kayjxP39kFQ").unwrap();
    // load bytecode
    // you can check the source code of the following wasm file in massa-sc-examples
    let bytecode = include_bytes!("./wasm/nested_call.wasm");
    // create the block containing the smart contract execution operation
    let operation = create_execute_sc_operation(&keypair, bytecode).unwrap();
    storage.store_operations(vec![operation.clone()]);
    let block = create_block(KeyPair::generate(), vec![operation], Slot::new(1, 0)).unwrap();
    // store the block in storage
    storage.store_block(block.clone());

    // set our block as a final block so the message is sent
    let mut finalized_blocks: HashMap<Slot, (BlockId, Storage)> = Default::default();
    finalized_blocks.insert(
        block.content.header.content.slot,
        (block.id, storage.clone()),
    );
    controller.update_blockclique_status(finalized_blocks.clone(), Default::default());
    std::thread::sleep(Duration::from_millis(10));

    // length of the sub contract test.wasm
    let bytecode_sub_contract_len = 5297;
    assert_eq!(
        sample_state
            .read()
            .ledger
            .get_balance(&Address::from_public_key(&keypair.get_public_key()))
            .unwrap(),
        Amount::from_str("300000")
            .unwrap()
            // Gas fee
            .saturating_sub(Amount::from_str("100000").unwrap())
            // Storage cost base
            .saturating_sub(
                exec_cfg
                    .storage_costs_constants
                    .ledger_cost_per_byte
                    .saturating_mul_u64(LEDGER_ENTRY_BASE_SIZE as u64)
            )
            // Storage bytecode key
            .saturating_sub(
                exec_cfg
                    .storage_costs_constants
                    .ledger_cost_per_byte
                    .saturating_mul_u64(ADDRESS_SIZE_BYTES as u64)
            )
            // Storage cost bytecode
            .saturating_sub(
                exec_cfg
                    .storage_costs_constants
                    .ledger_cost_per_byte
                    .saturating_mul_u64(bytecode_sub_contract_len)
            )
    );
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
    let mut storage = Storage::create_root();
    storage.store_operations(vec![operation.clone()]);
    let block = create_block(KeyPair::generate(), vec![operation], Slot::new(1, 1)).unwrap();
    // store the block in storage
    storage.store_block(block.clone());
    // set our block as a final block so the message is sent
    let mut finalized_blocks: HashMap<Slot, (BlockId, Storage)> = Default::default();
    finalized_blocks.insert(
        block.content.header.content.slot,
        (block.id, storage.clone()),
    );
    controller.update_blockclique_status(finalized_blocks, Default::default());
    std::thread::sleep(Duration::from_millis(10));
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
    // setup the period duration and the maximum gas for asynchronous messages execution
    let exec_cfg = ExecutionConfig {
        t0: 100.into(),
        max_async_gas: 100_000,
        ..ExecutionConfig::default()
    };
    // get a sample final state
    let (sample_state, _keep_file, _keep_dir) = get_sample_state().unwrap();

    // init the storage
    let mut storage = Storage::create_root();
    // start the execution worker
    let (mut manager, controller) = start_execution_worker(
        exec_cfg,
        sample_state.clone(),
        sample_state.read().pos_state.selector.clone(),
    );
    // keypair associated to thread 0
    let keypair = KeyPair::from_str("S1JJeHiZv1C1zZN5GLFcbz6EXYiccmUPLkYuDFA3kayjxP39kFQ").unwrap();
    // load send_message bytecode
    // you can check the source code of the following wasm file in massa-sc-examples
    let bytecode = include_bytes!("./wasm/send_message.wasm");
    // create the block contaning the smart contract execution operation
    let operation = create_execute_sc_operation(&keypair, bytecode).unwrap();
    storage.store_operations(vec![operation.clone()]);
    let block = create_block(KeyPair::generate(), vec![operation], Slot::new(1, 0)).unwrap();
    // store the block in storage
    storage.store_block(block.clone());

    // set our block as a final block so the message is sent
    let mut finalized_blocks: HashMap<Slot, (BlockId, Storage)> = Default::default();
    finalized_blocks.insert(
        block.content.header.content.slot,
        (block.id, storage.clone()),
    );
    controller.update_blockclique_status(finalized_blocks, Default::default());
    // sleep for 100ms to reach the message execution period
    std::thread::sleep(Duration::from_millis(100));

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
    // setup the period duration
    let exec_cfg = ExecutionConfig {
        t0: 100.into(),
        ..ExecutionConfig::default()
    };
    // get a sample final state
    let (sample_state, _keep_file, _keep_dir) = get_sample_state().unwrap();

    // init the storage
    let mut storage = Storage::create_root();
    // start the execution worker
    let (mut manager, controller) = start_execution_worker(
        exec_cfg.clone(),
        sample_state.clone(),
        sample_state.read().pos_state.selector.clone(),
    );
    // generate the sender_keypair and recipient_address
    let sender_keypair =
        KeyPair::from_str("S1JJeHiZv1C1zZN5GLFcbz6EXYiccmUPLkYuDFA3kayjxP39kFQ").unwrap();
    let (recipient_address, _keypair) = get_random_address_full();
    // create the operation
    let operation = Operation::new_wrapped(
        Operation {
            fee: Amount::zero(),
            expire_period: 10,
            op: OperationType::Transaction {
                recipient_address,
                amount: Amount::from_str("100").unwrap(),
            },
        },
        OperationSerializer::new(),
        &sender_keypair,
    )
    .unwrap();
    // create the block containing the transaction operation
    storage.store_operations(vec![operation.clone()]);
    let block = create_block(KeyPair::generate(), vec![operation], Slot::new(1, 0)).unwrap();
    // store the block in storage
    storage.store_block(block.clone());
    // set our block as a final block so the transaction is processed
    let mut finalized_blocks: HashMap<Slot, (BlockId, Storage)> = Default::default();
    finalized_blocks.insert(
        block.content.header.content.slot,
        (block.id, storage.clone()),
    );
    controller.update_blockclique_status(finalized_blocks, Default::default());
    std::thread::sleep(Duration::from_millis(10));
    // check recipient balance
    assert_eq!(
        sample_state
            .read()
            .ledger
            .get_balance(&recipient_address)
            .unwrap(),
        // Storage cost applied
        Amount::from_str("100")
            .unwrap()
            // Storage cost base
            .saturating_sub(
                exec_cfg
                    .storage_costs_constants
                    .ledger_cost_per_byte
                    .saturating_mul_u64(LEDGER_ENTRY_BASE_SIZE as u64)
            )
    );
    // stop the execution controller
    manager.stop();
}

#[test]
#[serial]
pub fn roll_buy() {
    // setup the period duration
    let exec_cfg = ExecutionConfig {
        t0: 100.into(),
        ..ExecutionConfig::default()
    };
    // get a sample final state
    let (sample_state, _keep_file, _keep_dir) = get_sample_state().unwrap();

    // init the storage
    let mut storage = Storage::create_root();
    // start the execution worker
    let (mut manager, controller) = start_execution_worker(
        exec_cfg,
        sample_state.clone(),
        sample_state.read().pos_state.selector.clone(),
    );
    // generate the keypair and its corresponding address
    let keypair = KeyPair::from_str("S1JJeHiZv1C1zZN5GLFcbz6EXYiccmUPLkYuDFA3kayjxP39kFQ").unwrap();
    let address = Address::from_public_key(&keypair.get_public_key());
    // create the operation
    let operation = Operation::new_wrapped(
        Operation {
            fee: Amount::zero(),
            expire_period: 10,
            op: OperationType::RollBuy { roll_count: 10 },
        },
        OperationSerializer::new(),
        &keypair,
    )
    .unwrap();
    // create the block contaning the roll buy operation
    storage.store_operations(vec![operation.clone()]);
    let block = create_block(KeyPair::generate(), vec![operation], Slot::new(1, 0)).unwrap();
    // store the block in storage
    storage.store_block(block.clone());
    // set our block as a final block so the purchase is processed
    let mut finalized_blocks: HashMap<Slot, (BlockId, Storage)> = Default::default();
    finalized_blocks.insert(
        block.content.header.content.slot,
        (block.id, storage.clone()),
    );
    controller.update_blockclique_status(finalized_blocks, Default::default());
    std::thread::sleep(Duration::from_millis(10));
    // check roll count of the buyer address and its balance
    let sample_read = sample_state.read();
    assert_eq!(sample_read.pos_state.get_rolls_for(&address), 110);
    assert_eq!(
        sample_read.ledger.get_balance(&address).unwrap(),
        Amount::from_str("299_000").unwrap()
    );
    // stop the execution controller
    manager.stop();
}

#[test]
#[serial]
pub fn roll_sell() {
    // setup the period duration
    let exec_cfg = ExecutionConfig {
        t0: 2.into(),
        periods_per_cycle: 2,
        thread_count: 2,
        ..Default::default()
    };
    // get a sample final state
    let (sample_state, _keep_file, _keep_dir) = get_sample_state().unwrap();

    // init the storage
    let mut storage = Storage::create_root();
    // start the execution worker
    let (mut manager, controller) = start_execution_worker(
        exec_cfg,
        sample_state.clone(),
        sample_state.read().pos_state.selector.clone(),
    );
    // generate the keypair and its corresponding address
    let keypair = KeyPair::from_str("S1JJeHiZv1C1zZN5GLFcbz6EXYiccmUPLkYuDFA3kayjxP39kFQ").unwrap();
    let address = Address::from_public_key(&keypair.get_public_key());
    // create the operation
    let operation = Operation::new_wrapped(
        Operation {
            fee: Amount::zero(),
            expire_period: 10,
            op: OperationType::RollSell { roll_count: 10 },
        },
        OperationSerializer::new(),
        &keypair,
    )
    .unwrap();
    // create the block contaning the roll buy operation
    storage.store_operations(vec![operation.clone()]);
    let block = create_block(KeyPair::generate(), vec![operation], Slot::new(1, 0)).unwrap();
    // store the block in storage
    storage.store_block(block.clone());
    // set the block as final so the sell and credits are processed
    let mut finalized_blocks: HashMap<Slot, (BlockId, Storage)> = Default::default();
    finalized_blocks.insert(
        block.content.header.content.slot,
        (block.id, storage.clone()),
    );
    controller.update_blockclique_status(finalized_blocks, Default::default());
    std::thread::sleep(Duration::from_millis(10));
    // check roll count deferred credits and candidate balance of the seller address
    let sample_read = sample_state.read();
    let mut credits = PreHashMap::default();
    credits.insert(address, Amount::from_str("1000").unwrap());
    assert_eq!(sample_read.pos_state.get_rolls_for(&address), 90);
    assert_eq!(
        sample_read
            .pos_state
            .get_deferred_credits_at(&Slot::new(7, 1)),
        credits
    );
    assert_eq!(
        controller.get_final_and_candidate_balance(&[address]),
        vec![(
            Some(Amount::from_str("300_000").unwrap()),
            Some(Amount::from_str("309_000").unwrap())
        )]
    );
    // stop the execution controller
    manager.stop();
}

#[test]
#[serial]
pub fn missed_blocks_roll_slash() {
    // setup the period duration
    let exec_cfg = ExecutionConfig {
        t0: 2.into(),
        periods_per_cycle: 2,
        thread_count: 2,
        ..Default::default()
    };
    // get a sample final state and make selections
    let (sample_state, _keep_file, _keep_dir) = get_sample_state().unwrap();

    // start the execution worker
    let (mut manager, controller) = start_execution_worker(
        exec_cfg,
        sample_state.clone(),
        sample_state.read().pos_state.selector.clone(),
    );
    // sleep to get slashed on missed blocks and reach the reimbursment
    std::thread::sleep(Duration::from_millis(100));
    // get the initial selection address
    let keypair = KeyPair::from_str("S1JJeHiZv1C1zZN5GLFcbz6EXYiccmUPLkYuDFA3kayjxP39kFQ").unwrap();
    let address = Address::from_public_key(&keypair.get_public_key());
    // check its balances
    assert_eq!(
        controller.get_final_and_candidate_balance(&[address]),
        vec![(
            Some(Amount::from_str("300_000").unwrap()),
            Some(Amount::from_str("310_000").unwrap())
        )]
    );
    // stop the execution controller
    manager.stop();
}

#[test]
#[serial]
fn sc_execution_error() {
    // setup the period duration and the maximum gas for asynchronous messages execution
    let exec_cfg = ExecutionConfig {
        t0: 100.into(),
        max_async_gas: 100_000,
        ..ExecutionConfig::default()
    };
    // get a sample final state
    let (sample_state, _keep_file, _keep_dir) = get_sample_state().unwrap();

    // init the storage
    let mut storage = Storage::create_root();
    // start the execution worker
    let (mut manager, controller) = start_execution_worker(
        exec_cfg,
        sample_state.clone(),
        sample_state.read().pos_state.selector.clone(),
    );
    // keypair associated to thread 0
    let keypair = KeyPair::from_str("S1JJeHiZv1C1zZN5GLFcbz6EXYiccmUPLkYuDFA3kayjxP39kFQ").unwrap();
    // load bytecode
    // you can check the source code of the following wasm file in massa-sc-examples
    let bytecode = include_bytes!("./wasm/execution_error.wasm");
    // create the block contaning the erroneous smart contract execution operation
    let operation = create_execute_sc_operation(&keypair, bytecode).unwrap();
    storage.store_operations(vec![operation.clone()]);
    let block = create_block(KeyPair::generate(), vec![operation], Slot::new(1, 0)).unwrap();
    // store the block in storage
    storage.store_block(block.clone());
    // set our block as a final block
    let mut finalized_blocks: HashMap<Slot, (BlockId, Storage)> = Default::default();
    finalized_blocks.insert(
        block.content.header.content.slot,
        (block.id, storage.clone()),
    );
    controller.update_blockclique_status(finalized_blocks, Default::default());
    std::thread::sleep(Duration::from_millis(10));

    // retrieve the event emitted by the execution error
    let events = controller.get_filtered_sc_output_event(EventFilter::default());
    // match the events
    assert!(!events.is_empty(), "One event was expected");
    assert!(events[0].data.contains("massa_execution_error"));
    assert!(events[0]
        .data
        .contains("runtime error when executing operation"));
    assert!(events[0].data.contains("address parsing error"));
    // stop the execution controller
    manager.stop();
}

#[test]
#[serial]
fn sc_datastore() {
    // setup the period duration and the maximum gas for asynchronous messages execution
    let exec_cfg = ExecutionConfig {
        t0: 100.into(),
        max_async_gas: 100_000,
        ..ExecutionConfig::default()
    };
    // get a sample final state
    let (sample_state, _keep_file, _keep_dir) = get_sample_state().unwrap();

    // init the storage
    let mut storage = Storage::create_root();
    // start the execution worker
    let (mut manager, controller) = start_execution_worker(
        exec_cfg,
        sample_state.clone(),
        sample_state.read().pos_state.selector.clone(),
    );
    // keypair associated to thread 0
    let keypair = KeyPair::from_str("S1JJeHiZv1C1zZN5GLFcbz6EXYiccmUPLkYuDFA3kayjxP39kFQ").unwrap();
    // load bytecode
    // you can check the source code of the following wasm file in massa-sc-examples
    let bytecode = include_bytes!("./wasm/datastore.wasm");

    let datastore = BTreeMap::from([(vec![65, 66], vec![255]), (vec![9], vec![10, 11])]);

    // create the block contaning the erroneous smart contract execution operation
    let operation =
        create_execute_sc_operation_with_datastore(&keypair, bytecode, datastore).unwrap();
    storage.store_operations(vec![operation.clone()]);
    let block = create_block(KeyPair::generate(), vec![operation], Slot::new(1, 0)).unwrap();
    // store the block in storage
    storage.store_block(block.clone());
    // set our block as a final block
    let mut finalized_blocks: HashMap<Slot, (BlockId, Storage)> = Default::default();
    finalized_blocks.insert(
        block.content.header.content.slot,
        (block.id, storage.clone()),
    );
    controller.update_blockclique_status(finalized_blocks, Default::default());
    std::thread::sleep(Duration::from_millis(10));

    // retrieve the event emitted by the execution error
    let events = controller.get_filtered_sc_output_event(EventFilter::default());

    // match the events
    assert!(!events.is_empty(), "One event was expected");
    assert_eq!(events[0].data, "keys: 9,65,66");
    assert_eq!(events[1].data, "has_key_1: true - has_key_2: false");
    assert_eq!(events[2].data, "data key 1: 255 - data key 3: 10,11");

    // stop the execution controller
    manager.stop();
}

#[test]
#[serial]
fn set_bytecode_error() {
    // setup the period duration and the maximum gas for asynchronous messages execution
    let exec_cfg = ExecutionConfig {
        t0: 100.into(),
        max_async_gas: 100_000,
        ..ExecutionConfig::default()
    };
    // get a sample final state
    let (sample_state, _keep_file, _keep_dir) = get_sample_state().unwrap();

    // init the storage
    let mut storage = Storage::create_root();
    // start the execution worker
    let (_manager, controller) = start_execution_worker(
        exec_cfg,
        sample_state.clone(),
        sample_state.read().pos_state.selector.clone(),
    );
    // keypair associated to thread 0
    let keypair = KeyPair::from_str("S1JJeHiZv1C1zZN5GLFcbz6EXYiccmUPLkYuDFA3kayjxP39kFQ").unwrap();
    // load bytecode
    // you can check the source code of the following wasm file in massa-sc-examples
    let bytecode = include_bytes!("./wasm/set_bytecode_fail.wasm");
    // create the block contaning the erroneous smart contract execution operation
    let operation = create_execute_sc_operation(&keypair, bytecode).unwrap();
    storage.store_operations(vec![operation.clone()]);
    let block = create_block(KeyPair::generate(), vec![operation], Slot::new(1, 0)).unwrap();
    // store the block in storage
    storage.store_block(block.clone());
    // set our block as a final block
    let mut finalized_blocks: HashMap<Slot, (BlockId, Storage)> = Default::default();
    finalized_blocks.insert(
        block.content.header.content.slot,
        (block.id, storage.clone()),
    );
    controller.update_blockclique_status(finalized_blocks, Default::default());
    std::thread::sleep(Duration::from_millis(10));

    // retrieve the event emitted by the execution error
    let events = controller.get_filtered_sc_output_event(EventFilter::default());
    // match the events
    assert!(!events.is_empty(), "One event was expected");
    assert!(events[0].data.contains("massa_execution_error"));
    assert!(events[0]
        .data
        .contains("runtime error when executing operation"));
    assert!(events[0].data.contains("can't set the bytecode of address"));
}

#[test]
#[serial]
fn datastore_manipulations() {
    // setup the period duration and the maximum gas for asynchronous messages execution
    let exec_cfg = ExecutionConfig {
        t0: 100.into(),
        max_async_gas: 100_000,
        ..ExecutionConfig::default()
    };
    // get a sample final state
    let (sample_state, _keep_file, _keep_dir) = get_sample_state().unwrap();

    // init the storage
    let mut storage = Storage::create_root();
    // start the execution worker
    let (mut manager, controller) = start_execution_worker(
        exec_cfg.clone(),
        sample_state.clone(),
        sample_state.read().pos_state.selector.clone(),
    );
    // keypair associated to thread 0
    let keypair = KeyPair::from_str("S1JJeHiZv1C1zZN5GLFcbz6EXYiccmUPLkYuDFA3kayjxP39kFQ").unwrap();
    // load bytecode
    // you can check the source code of the following wasm file in massa-sc-examples
    let bytecode = include_bytes!("./wasm/datastore_manipulations.wasm");
    // create the block contaning the erroneous smart contract execution operation
    let operation = create_execute_sc_operation(&keypair, bytecode).unwrap();
    storage.store_operations(vec![operation.clone()]);
    let block = create_block(KeyPair::generate(), vec![operation], Slot::new(1, 0)).unwrap();
    // store the block in storage
    storage.store_block(block.clone());
    // set our block as a final block
    let mut finalized_blocks: HashMap<Slot, (BlockId, Storage)> = Default::default();
    finalized_blocks.insert(
        block.content.header.content.slot,
        (block.id, storage.clone()),
    );
    controller.update_blockclique_status(finalized_blocks, Default::default());
    std::thread::sleep(Duration::from_millis(10));

    // Length of the value left in the datastore. See sources for more context.
    let value_len = 10;
    assert_eq!(
        sample_state
            .read()
            .ledger
            .get_balance(&Address::from_public_key(&keypair.get_public_key()))
            .unwrap(),
        Amount::from_str("300000")
            .unwrap()
            // Gas fee
            .saturating_sub(Amount::from_str("100000").unwrap())
            // Storage cost key
            .saturating_sub(
                exec_cfg
                    .storage_costs_constants
                    .ledger_cost_per_byte
                    .saturating_mul_u64(LEDGER_ENTRY_DATASTORE_BASE_SIZE as u64)
            )
            // Storage cost value
            .saturating_sub(
                exec_cfg
                    .storage_costs_constants
                    .ledger_cost_per_byte
                    .saturating_mul_u64(value_len)
            )
    );

    // stop the execution controller
    manager.stop();
}

#[test]
#[serial]
fn generate_events() {
    // Compile the `./wasm_tests` and generate a block with `event_test.wasm`
    // as data. Then we check if we get an event as expected.
    let exec_cfg = ExecutionConfig {
        t0: 100.into(),
        ..ExecutionConfig::default()
    };
    let mut storage: Storage = Storage::create_root();
    let (sample_state, _keep_file, _keep_dir) = get_sample_state().unwrap();
    let (mut manager, controller) = start_execution_worker(
        exec_cfg,
        sample_state.clone(),
        sample_state.read().pos_state.selector.clone(),
    );

    let keypair = KeyPair::from_str("S1JJeHiZv1C1zZN5GLFcbz6EXYiccmUPLkYuDFA3kayjxP39kFQ").unwrap();
    let sender_address = Address::from_public_key(&keypair.get_public_key());
    let event_test_data = include_bytes!("./wasm/event_test.wasm");
    let operation = create_execute_sc_operation(&keypair, event_test_data).unwrap();
    storage.store_operations(vec![operation.clone()]);
    let block = create_block(keypair, vec![operation], Slot::new(1, 0)).unwrap();
    let slot = block.content.header.content.slot;

    storage.store_block(block.clone());

    let finalized_blocks: HashMap<Slot, (BlockId, Storage)> = HashMap::new();
    let mut blockclique: HashMap<Slot, (BlockId, Storage)> = HashMap::new();

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
        gas_price: Amount::from_mantissa_scale(1, 0),
        datastore: BTreeMap::new(),
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

/// Create an operation for the given sender with `data` as bytecode.
/// Return a result that should be unwrapped in the root `#[test]` routine.
fn create_execute_sc_operation_with_datastore(
    sender_keypair: &KeyPair,
    data: &[u8],
    datastore: Datastore,
) -> Result<WrappedOperation, ExecutionError> {
    let op = OperationType::ExecuteSC {
        data: data.to_vec(),
        max_gas: 100_000,
        gas_price: Amount::from_mantissa_scale(1, 0),
        datastore: datastore,
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
        coins: Amount::from_str("0").unwrap(),
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
