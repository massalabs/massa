// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_db_exports::DBBatch;
use massa_execution_exports::{
    ExecutionBlockMetadata, ExecutionConfig, ExecutionQueryRequest, ExecutionQueryRequestItem,
    ReadOnlyExecutionRequest, ReadOnlyExecutionTarget,
};
use massa_models::config::{
    ENDORSEMENT_COUNT, LEDGER_ENTRY_BASE_COST, LEDGER_ENTRY_DATASTORE_BASE_SIZE,
};
use massa_models::prehash::PreHashMap;
use massa_models::test_exports::gen_endorsements_for_denunciation;
use massa_models::{address::Address, amount::Amount, slot::Slot};
use massa_models::{
    block_id::BlockId,
    denunciation::Denunciation,
    execution::EventFilter,
    operation::{Operation, OperationSerializer, OperationType},
    secure_share::SecureShareContent,
};
use massa_pos_exports::{MockSelectorControllerWrapper, Selection};
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_test_framework::TestUniverse;
use massa_time::MassaTime;
use num::rational::Ratio;
use std::{
    cmp::Reverse, collections::BTreeMap, collections::HashMap, str::FromStr, time::Duration,
};

use super::universe::{ExecutionForeignControllers, ExecutionTestUniverse};

const TEST_SK_1: &str = "S18r2i8oJJyhF7Kprx98zwxAc3W4szf7RKuVMX6JydZz8zSxHeC";
const TEST_SK_2: &str = "S1FpYC4ugG9ivZZbLVrTwWtF9diSRiAwwrVX5Gx1ANSRLfouUjq";
const TEST_SK_3: &str = "S1LgXhWLEgAgCX3nm6y8PVPzpybmsYpi6yg6ZySwu5Z4ERnD7Bu";

fn selector_boilerplate(
    mock_selector: &mut MockSelectorControllerWrapper,
    block_creator: &KeyPair,
) {
    let block_creator_address = Address::from_public_key(&block_creator.get_public_key());
    mock_selector.set_expectations(|selector_controller| {
        selector_controller
            .expect_feed_cycle()
            .returning(move |_, _, _| Ok(()));
        selector_controller
            .expect_wait_for_draws()
            .returning(move |cycle| Ok(cycle + 1));
        selector_controller
            .expect_get_producer()
            .returning(move |_| Ok(block_creator_address));
        selector_controller
            .expect_get_selection()
            .returning(move |_| {
                Ok(Selection {
                    producer: block_creator_address,
                    endorsements: vec![block_creator_address; ENDORSEMENT_COUNT as usize],
                })
            });
    });
}

#[test]
fn test_execution_shutdown() {
    let block_producer = KeyPair::generate(0).unwrap();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(
        &mut foreign_controllers.selector_controller,
        &block_producer,
    );
    ExecutionTestUniverse::new(foreign_controllers, ExecutionConfig::default());
    std::thread::sleep(Duration::from_millis(100));
}

#[test]
fn test_sending_command() {
    let block_producer = KeyPair::generate(0).unwrap();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(
        &mut foreign_controllers.selector_controller,
        &block_producer,
    );
    let universe = ExecutionTestUniverse::new(foreign_controllers, ExecutionConfig::default());
    universe.module_controller.update_blockclique_status(
        Default::default(),
        Default::default(),
        Default::default(),
    );
    std::thread::sleep(Duration::from_millis(100));
}

#[test]
fn test_readonly_execution() {
    // setup the period duration
    let exec_cfg = ExecutionConfig {
        t0: MassaTime::from_millis(100),
        cursor_delay: MassaTime::from_millis(0),
        ..ExecutionConfig::default()
    };
    let block_producer = KeyPair::generate(0).unwrap();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(
        &mut foreign_controllers.selector_controller,
        &block_producer,
    );
    let universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);

    let mut res = universe
        .module_controller
        .execute_readonly_request(ReadOnlyExecutionRequest {
            max_gas: 1_000_000,
            call_stack: vec![],
            target: ReadOnlyExecutionTarget::BytecodeExecution(
                include_bytes!("./wasm/event_test.wasm").to_vec(),
            ),
            is_final: true,
            coins: None,
            fee: None,
        })
        .expect("readonly execution failed");

    assert_eq!(res.out.slot, Slot::new(1, 0));
    assert!(res.gas_cost > 0);
    assert_eq!(res.out.events.take().len(), 1, "wrong number of events");
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
fn test_nested_call_gas_usage() {
    // setup the period duration
    let exec_cfg = ExecutionConfig {
        t0: MassaTime::from_millis(100),
        cursor_delay: MassaTime::from_millis(0),
        ..ExecutionConfig::default()
    };
    let block_producer = KeyPair::generate(0).unwrap();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(
        &mut foreign_controllers.selector_controller,
        &block_producer,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);

    // get random keypair
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
    // load bytecodes
    // you can check the source code of the following wasm file in massa-unit-tests-src
    let bytecode = include_bytes!("./wasm/nested_call.wasm");
    let datastore_bytecode = include_bytes!("./wasm/test.wasm").to_vec();
    let mut datastore = BTreeMap::new();
    datastore.insert(b"smart-contract".to_vec(), datastore_bytecode);

    // create the block containing the smart contract execution operation
    let operation =
        ExecutionTestUniverse::create_execute_sc_operation(&keypair, bytecode, datastore).unwrap();
    universe.storage.store_operations(vec![operation.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &block_producer,
        Slot::new(1, 0),
        vec![operation],
        vec![],
        vec![],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());

    // set our block as a final block so the message is sent
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
    block_metadata.insert(
        block.id,
        ExecutionBlockMetadata {
            same_thread_parent_creator: Some(Address::from_public_key(&keypair.get_public_key())),
            storage: Some(universe.storage.clone()),
        },
    );
    universe.module_controller.update_blockclique_status(
        finalized_blocks.clone(),
        Default::default(),
        block_metadata.clone(),
    );
    std::thread::sleep(Duration::from_millis(100));
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter {
            start: Some(Slot::new(0, 1)),
            end: Some(Slot::new(20, 1)),
            ..Default::default()
        });
    // match the events
    assert!(!events.is_empty(), "One event was expected");
    let address = events[0].clone().data;
    // Call the function test of the smart contract
    let operation = ExecutionTestUniverse::create_call_sc_operation(
        &keypair,
        10000000,
        Amount::from_str("0").unwrap(),
        Amount::from_str("0").unwrap(),
        Address::from_str(&address).unwrap(),
        String::from("test"),
        address.as_bytes().to_vec(),
    )
    .unwrap();
    // Init new storage for this block
    let mut storage = Storage::create_root();
    storage.store_operations(vec![operation.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &block_producer,
        Slot::new(2, 0),
        vec![operation],
        vec![],
        vec![],
    );
    // store the block in storage
    storage.store_block(block.clone());
    // set our block as a final block so the message is sent
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
    block_metadata.insert(
        block.id,
        ExecutionBlockMetadata {
            same_thread_parent_creator: Some(Address::from_public_key(&keypair.get_public_key())),
            storage: Some(storage.clone()),
        },
    );
    universe.module_controller.update_blockclique_status(
        finalized_blocks,
        Default::default(),
        block_metadata.clone(),
    );
    std::thread::sleep(Duration::from_millis(100));
    // Get the events that give us the gas usage (refer to source in ts) without fetching the first slot because it emit a event with an address.
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter {
            start: Some(Slot::new(2, 0)),
            ..Default::default()
        });
    assert!(!events.is_empty());
    // Check that we always subtract gas through the execution (even in sub calls)
    let events_formatted = events
        .iter()
        .map(|event| event.data.parse::<u64>().unwrap())
        .collect::<Vec<_>>();
    let mut sorted_events = events_formatted.clone();
    sorted_events.sort_by_key(|event| Reverse(*event));
    assert_eq!(
        events_formatted, sorted_events,
        "Gas is not going down through the execution."
    );
}

/// Test the ABI get call coins
///
/// Deploy an SC with a method `test` that generate an event saying how many coins he received
/// Calling the SC in a second time
#[test]
fn test_get_call_coins() {
    // setup the period duration
    let exec_cfg = ExecutionConfig {
        t0: MassaTime::from_millis(100),
        cursor_delay: MassaTime::from_millis(0),
        ..ExecutionConfig::default()
    };
    let block_producer = KeyPair::generate(0).unwrap();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();

    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(
        &mut foreign_controllers.selector_controller,
        &block_producer,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);

    // load bytecodes
    // you can check the source code of the following wasm file in massa-unit-tests-src
    let bytecode = include_bytes!("./wasm/get_call_coins_main.wasm");
    let datastore_bytecode = include_bytes!("./wasm/get_call_coins_test.wasm").to_vec();
    let mut datastore = BTreeMap::new();
    datastore.insert(b"smart-contract".to_vec(), datastore_bytecode);

    // create the block containing the smart contract execution operation
    let operation =
        ExecutionTestUniverse::create_execute_sc_operation(&keypair, bytecode, datastore).unwrap();
    universe.storage.store_operations(vec![operation.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &keypair,
        Slot::new(1, 0),
        vec![operation],
        vec![],
        vec![],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());

    // set our block as a final block so the message is sent
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
    block_metadata.insert(
        block.id,
        ExecutionBlockMetadata {
            storage: Some(universe.storage.clone()),
            same_thread_parent_creator: Some(Address::from_public_key(&keypair.get_public_key())),
        },
    );
    universe.module_controller.update_blockclique_status(
        finalized_blocks.clone(),
        Default::default(),
        block_metadata.clone(),
    );

    std::thread::sleep(Duration::from_millis(100));

    // assert_eq!(balance, balance_expected);
    // retrieve events emitted by smart contracts
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter {
            start: Some(Slot::new(0, 1)),
            end: Some(Slot::new(20, 1)),
            ..Default::default()
        });
    // match the events
    assert!(!events.is_empty(), "One event was expected");
    let address = events[0].clone().data;
    // Call the function test of the smart contract
    let coins_sent = Amount::from_str("10").unwrap();
    let operation = ExecutionTestUniverse::create_call_sc_operation(
        &keypair,
        10000000,
        Amount::from_str("0").unwrap(),
        coins_sent,
        Address::from_str(&address).unwrap(),
        String::from("test"),
        address.as_bytes().to_vec(),
    )
    .unwrap();
    // Init new storage for this block
    universe.storage.store_operations(vec![operation.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &keypair,
        Slot::new(2, 0),
        vec![operation],
        vec![],
        vec![],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());
    // set our block as a final block so the message is sent
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
    block_metadata.insert(
        block.id,
        ExecutionBlockMetadata {
            same_thread_parent_creator: Some(Address::from_public_key(&keypair.get_public_key())),
            storage: Some(universe.storage.clone()),
        },
    );
    universe.module_controller.update_blockclique_status(
        finalized_blocks,
        Default::default(),
        block_metadata.clone(),
    );
    std::thread::sleep(Duration::from_millis(100));
    // Get the events that give us the gas usage (refer to source in ts) without fetching the first slot because it emit a event with an address.
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter {
            start: Some(Slot::new(2, 0)),
            ..Default::default()
        });
    println!("events {:#?}", events);
    assert!(events[0].data.contains(&format!(
        "tokens sent to the SC during the call : {}",
        coins_sent.to_raw()
    )));
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
fn send_and_receive_async_message() {
    let exec_cfg = ExecutionConfig {
        t0: MassaTime::from_millis(100),
        cursor_delay: MassaTime::from_millis(0),
        max_async_gas: 100_000,
        ..ExecutionConfig::default()
    };
    let block_producer = KeyPair::generate(0).unwrap();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();

    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(
        &mut foreign_controllers.selector_controller,
        &block_producer,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);

    // load bytecodes
    // you can check the source code of the following wasm file in massa-unit-tests-src
    let bytecode = include_bytes!("./wasm/send_message.wasm");
    let datastore_bytecode = include_bytes!("./wasm/receive_message.wasm").to_vec();
    let mut datastore = BTreeMap::new();
    datastore.insert(b"smart-contract".to_vec(), datastore_bytecode);

    // create the block contaning the smart contract execution operation
    let operation =
        ExecutionTestUniverse::create_execute_sc_operation(&keypair, bytecode, datastore).unwrap();
    universe.storage.store_operations(vec![operation.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &block_producer,
        Slot::new(1, 0),
        vec![operation],
        vec![],
        vec![],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());

    // set our block as a final block so the message is sent
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
    block_metadata.insert(
        block.id,
        ExecutionBlockMetadata {
            same_thread_parent_creator: Some(Address::from_public_key(&keypair.get_public_key())),
            storage: Some(universe.storage.clone()),
        },
    );
    universe.module_controller.update_blockclique_status(
        finalized_blocks,
        Default::default(),
        block_metadata.clone(),
    );
    // sleep for 150ms to reach the message execution period
    std::thread::sleep(Duration::from_millis(150));

    // retrieve events emitted by smart contracts
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter {
            start: Some(Slot::new(1, 1)),
            end: Some(Slot::new(20, 1)),
            ..Default::default()
        });

    println!("events: {:?}", events);

    // match the events
    assert!(events.len() == 1, "One event was expected");
    assert_eq!(events[0].data, "message correctly received: 42,42,42,42");
}

/// # Context
///
/// Mostly the same as send_and_receive_async_message
///
/// Functional test that tests the execution status of an operation is correctly recorded
///
/// 1. a block is created containing an `execute_sc` operation
/// 2. this operation executes the `send_message` of the smart contract
/// 3. `send_message` stores the `receive_message` of the smart contract on the block
/// 4. `receive_message` contains the message handler function
/// 5. `send_message` sends a message to the `receive_message` address
/// 6. we set the created block as finalized so the message is actually sent
/// 7. we execute the following slots for 300 milliseconds to reach the message execution period
/// 8. once the execution period is over we stop the execution controller
/// 9. we retrieve the status of the executed operation(s)
/// 10 we check that the monitored operation has been executed
/// 11 we check that the execution status is the one we expected
///
#[test]
fn test_operation_execution_status() {
    // setup the period duration
    let exec_cfg = ExecutionConfig {
        t0: MassaTime::from_millis(100),
        cursor_delay: MassaTime::from_millis(0),
        ..ExecutionConfig::default()
    };
    let block_producer = KeyPair::generate(0).unwrap();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();

    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(
        &mut foreign_controllers.selector_controller,
        &block_producer,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);
    // load bytecodes
    // you can check the source code of the following wasm file in massa-unit-tests-src
    let bytecode = include_bytes!("./wasm/send_message.wasm");
    let datastore_bytecode = include_bytes!("./wasm/receive_message.wasm").to_vec();
    let mut datastore = BTreeMap::new();
    datastore.insert(b"smart-contract".to_vec(), datastore_bytecode);

    // create the block contaning the smart contract execution operation
    let operation =
        ExecutionTestUniverse::create_execute_sc_operation(&keypair, bytecode, datastore).unwrap();
    let tested_op_id = operation.id;
    universe.storage.store_operations(vec![operation.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &block_producer,
        Slot::new(1, 0),
        vec![operation],
        vec![],
        vec![],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());

    // set our block as a final block so the message is sent
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
    block_metadata.insert(
        block.id,
        ExecutionBlockMetadata {
            same_thread_parent_creator: Some(Address::from_public_key(&keypair.get_public_key())),
            storage: Some(universe.storage.clone()),
        },
    );
    universe.module_controller.update_blockclique_status(
        finalized_blocks,
        Default::default(),
        block_metadata.clone(),
    );
    // sleep for 150ms to reach the message execution period
    std::thread::sleep(Duration::from_millis(150));

    let (op_candidate, op_final) = universe
        .module_controller
        .get_ops_exec_status(&[tested_op_id])[0];

    // match the events
    assert!(
        op_candidate == Some(true) && op_final == Some(true),
        "Expected operation not found or not successfully executed"
    );
}

/// Context
///
/// Functional test for local smart-contract execution
///
/// 1. a block is created with 2 ExecuteSC operations
///    it contains 1 local execution and 1 local call
///    both operation datastores have the bytecode of local_function.wasm
/// 2. store and set the block as final
/// 3. wait for execution
/// 4. retrieve events emitted by the initial an sub functions
/// 5. match event and call stack to make sure that executions were local
#[test]
fn local_execution() {
    // setup the period duration
    let exec_cfg = ExecutionConfig {
        t0: MassaTime::from_millis(100),
        cursor_delay: MassaTime::from_millis(0),
        ..ExecutionConfig::default()
    };
    let block_producer = KeyPair::generate(0).unwrap();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();

    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(
        &mut foreign_controllers.selector_controller,
        &block_producer,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);
    // load bytecodes
    // you can check the source code of the following wasm files in massa-unit-tests-src
    let exec_bytecode = include_bytes!("./wasm/local_execution.wasm");
    let call_bytecode = include_bytes!("./wasm/local_call.wasm");
    let datastore_bytecode = include_bytes!("./wasm/local_function.wasm").to_vec();
    let mut datastore = BTreeMap::new();
    datastore.insert(b"smart-contract".to_vec(), datastore_bytecode);

    // create the block contaning the operations
    let local_exec_op = ExecutionTestUniverse::create_execute_sc_operation(
        &keypair,
        exec_bytecode,
        datastore.clone(),
    )
    .unwrap();
    let local_call_op =
        ExecutionTestUniverse::create_execute_sc_operation(&keypair, call_bytecode, datastore)
            .unwrap();
    universe
        .storage
        .store_operations(vec![local_exec_op.clone(), local_call_op.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &block_producer,
        Slot::new(1, 0),
        vec![local_exec_op, local_call_op],
        vec![],
        vec![],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());

    // set our block as a final block so the message is sent
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
    block_metadata.insert(
        block.id,
        ExecutionBlockMetadata {
            same_thread_parent_creator: Some(Address::from_public_key(&keypair.get_public_key())),
            storage: Some(universe.storage.clone()),
        },
    );
    universe.module_controller.update_blockclique_status(
        finalized_blocks,
        Default::default(),
        block_metadata.clone(),
    );
    // sleep for 100ms to wait for execution
    std::thread::sleep(Duration::from_millis(100));

    // retrieve events emitted by smart contracts
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter {
            ..Default::default()
        });

    // match the events, check balance and call stack to make sure the executions were local
    assert!(events.len() == 8, "8 events were expected");
    assert_eq!(
        Amount::from_raw(events[1].data.parse().unwrap()),
        Amount::from_str("299990").unwrap() // start (300_000) - fee (1000)
    );
    assert_eq!(events[1].context.call_stack.len(), 1);
    assert_eq!(
        events[1].context.call_stack.back().unwrap(),
        &Address::from_public_key(&keypair.get_public_key())
    );
    assert_eq!(events[2].data, "one local execution completed");
    let amount = Amount::from_raw(events[5].data.parse().unwrap());
    assert_eq!(Amount::from_str("299979.6713").unwrap(), amount);
    assert_eq!(events[5].context.call_stack.len(), 1);
    assert_eq!(
        events[1].context.call_stack.back().unwrap(),
        &Address::from_public_key(&keypair.get_public_key())
    );
    assert_eq!(events[6].data, "one local call completed");
}

/// Context
///
/// Functional test for sc deployment utility functions, `functionExists` and `callerHasWriteAccess`
///
/// 1. a block is created with one ExecuteSC operation containing
///    a deployment sc as bytecode to execute and a deplyed sc as an op datatsore entry
/// 2. store and set the block as final
/// 3. wait for execution
/// 4. retrieve events emitted by the initial an sub functions
/// 5. match events to make sure that `functionExists` and `callerHasWriteAccess` had the expected behaviour
#[test]
fn sc_deployment() {
    // setup the period duration
    let exec_cfg = ExecutionConfig {
        t0: MassaTime::from_millis(100),
        cursor_delay: MassaTime::from_millis(0),
        ..ExecutionConfig::default()
    };
    let block_producer = KeyPair::generate(0).unwrap();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();

    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(
        &mut foreign_controllers.selector_controller,
        &block_producer,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);
    // load bytecodes
    // you can check the source code of the following wasm files in massa-unit-tests-src
    let op_bytecode = include_bytes!("./wasm/deploy_sc.wasm");
    let datastore_bytecode = include_bytes!("./wasm/init_sc.wasm").to_vec();
    let mut datastore = BTreeMap::new();
    datastore.insert(b"smart-contract".to_vec(), datastore_bytecode);

    // create the block containing the operation
    let op = ExecutionTestUniverse::create_execute_sc_operation(
        &keypair,
        op_bytecode,
        datastore.clone(),
    )
    .unwrap();
    universe.storage.store_operations(vec![op.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &block_producer,
        Slot::new(1, 0),
        vec![op],
        vec![],
        vec![],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());

    // set our block as a final block so the message is sent
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
    block_metadata.insert(
        block.id,
        ExecutionBlockMetadata {
            same_thread_parent_creator: Some(Address::from_public_key(&keypair.get_public_key())),
            storage: Some(universe.storage.clone()),
        },
    );
    universe.module_controller.update_blockclique_status(
        finalized_blocks,
        Default::default(),
        block_metadata.clone(),
    );
    // sleep for 100ms to wait for execution
    std::thread::sleep(Duration::from_millis(100));

    // retrieve events emitted by smart contracts
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter {
            ..Default::default()
        });

    // match the events
    if events.len() != 3 {
        for (i, ev) in events.iter().enumerate() {
            eprintln!("ev {}: {}", i, ev);
        }
        panic!("3 events were expected");
    }
    assert_eq!(events[0].data, "sc created");
    assert_eq!(events[1].data, "constructor exists and will be called");
    assert_eq!(events[2].data, "constructor called by deployer");
}

/// # Context
///
/// Functional test for asynchronous messages sending and handling with a filter
///
/// 1. a block is created containing an `execute_sc` operation
/// 2. this operation deploy a smart contract and call his function `test`
/// 3. `test` generates an event and place a message to be triggered once again if `test2` datastore key of address `AS12DDxjqtBVshdQ4nLqYg6GwRddY5LzEC7bnatVxB5SFtpbCFj8E` is created/modify
/// 4. we set the created block as finalized so the message is actually sent
/// 5. we execute the following slots for 300 milliseconds to reach the message execution period
/// 6. We send a new operation with a smart contract that modify `test` datastore key and so doesn't trigger the message.
/// 7. We send a new operation with a smart contract that create `test2` datastore key and so trigger the message.
/// 8. once the execution period is over we stop the execution controller
/// 9. we retrieve the events emitted by smart contract
/// 10. `test` handler function should have emitted a second event
/// 11. we check if they are events
/// 12. if they are some, we verify that the data has the correct value
#[test]
fn send_and_receive_async_message_with_trigger() {
    // setup the period duration
    let exec_cfg = ExecutionConfig {
        t0: MassaTime::from_millis(100),
        cursor_delay: MassaTime::from_millis(0),
        max_async_gas: 1_000_000_000,
        ..ExecutionConfig::default()
    };
    let block_producer = KeyPair::generate(0).unwrap();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
    let mut blockclique_blocks: HashMap<Slot, BlockId> = HashMap::default();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(
        &mut foreign_controllers.selector_controller,
        &block_producer,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);
    // load bytecode
    // you can check the source code of the following wasm file in massa-unit-tests-src
    let bytecode = include_bytes!("./wasm/send_message_deploy_condition.wasm");
    let datastore_bytecode = include_bytes!("./wasm/send_message_condition.wasm").to_vec();
    let mut datastore = BTreeMap::new();
    let key = unsafe {
        String::from("smart-contract")
            .encode_utf16()
            .collect::<Vec<u16>>()
            .align_to::<u8>()
            .1
            .to_vec()
    };
    datastore.insert(key, datastore_bytecode);

    // create the block containing the smart contract execution operation
    let operation =
        ExecutionTestUniverse::create_execute_sc_operation(&keypair, bytecode, datastore).unwrap();
    universe.storage.store_operations(vec![operation.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &block_producer,
        Slot::new(1, 0),
        vec![operation],
        vec![],
        vec![],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());

    // set our block as a final block so the message is sent
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
    block_metadata.insert(
        block.id,
        ExecutionBlockMetadata {
            same_thread_parent_creator: Some(Address::from_public_key(&keypair.get_public_key())),
            storage: Some(universe.storage.clone()),
        },
    );
    blockclique_blocks.insert(block.content.header.content.slot, block.id);
    universe.module_controller.update_blockclique_status(
        finalized_blocks.clone(),
        Some(blockclique_blocks.clone()),
        block_metadata.clone(),
    );
    // sleep for 10ms to reach the message execution period
    std::thread::sleep(Duration::from_millis(10));

    // retrieve events emitted by smart contracts
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter {
            ..Default::default()
        });

    // match the events
    assert_eq!(events.len(), 2, "2 events were expected");
    assert_eq!(events[0].data, "Triggered");

    // keypair associated to thread 1
    let keypair = KeyPair::from_str(TEST_SK_2).unwrap();
    // load bytecode
    // you can check the source code of the following wasm file in massa-unit-tests-src
    let bytecode = include_bytes!("./wasm/send_message_wrong_trigger.wasm");
    let datastore = BTreeMap::new();

    // create the block containing the smart contract execution operation
    let operation =
        ExecutionTestUniverse::create_execute_sc_operation(&keypair, bytecode, datastore).unwrap();
    universe.storage.store_operations(vec![operation.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &block_producer,
        Slot::new(1, 1),
        vec![operation],
        vec![],
        vec![],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());

    // set our block as a final block so the message is sent
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
    block_metadata.insert(
        block.id,
        ExecutionBlockMetadata {
            same_thread_parent_creator: Some(Address::from_public_key(&keypair.get_public_key())),
            storage: Some(universe.storage.clone()),
        },
    );
    blockclique_blocks.insert(block.content.header.content.slot, block.id);
    universe.module_controller.update_blockclique_status(
        finalized_blocks.clone(),
        None,
        block_metadata.clone(),
    );
    // sleep for 10ms to reach the message execution period
    std::thread::sleep(Duration::from_millis(10));

    // retrieve events emitted by smart contracts
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter {
            ..Default::default()
        });

    // match the events
    assert!(events.len() == 3, "3 events were expected");

    // keypair associated to thread 2
    let keypair = KeyPair::from_str(TEST_SK_3).unwrap();
    // load bytecode
    // you can check the source code of the following wasm file in massa-unit-tests-src
    // This line execute the smart contract that will modify the data entry and then trigger the SC.
    let bytecode = include_bytes!("./wasm/send_message_trigger.wasm");
    let datastore = BTreeMap::new();

    let operation =
        ExecutionTestUniverse::create_execute_sc_operation(&keypair, bytecode, datastore).unwrap();
    universe.storage.store_operations(vec![operation.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &block_producer,
        Slot::new(1, 2),
        vec![operation],
        vec![],
        vec![],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());

    // set our block as a final block so the message is sent
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
    block_metadata.insert(
        block.id,
        ExecutionBlockMetadata {
            same_thread_parent_creator: Some(Address::from_public_key(&keypair.get_public_key())),
            storage: Some(universe.storage.clone()),
        },
    );
    blockclique_blocks.insert(block.content.header.content.slot, block.id);
    universe.module_controller.update_blockclique_status(
        finalized_blocks.clone(),
        None,
        block_metadata.clone(),
    );
    // sleep for 1000ms to reach the message execution period
    std::thread::sleep(Duration::from_millis(1000));

    // retrieve events emitted by smart contracts
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter {
            ..Default::default()
        });

    // match the events
    assert!(events.len() == 4, "4 events were expected");
}

#[test]
fn send_and_receive_transaction() {
    // setup the period duration
    let exec_cfg = ExecutionConfig {
        t0: MassaTime::from_millis(100),
        cursor_delay: MassaTime::from_millis(0),
        ..ExecutionConfig::default()
    };
    let block_producer = KeyPair::generate(0).unwrap();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();

    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(
        &mut foreign_controllers.selector_controller,
        &block_producer,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);
    let recipient_address =
        Address::from_public_key(&KeyPair::generate(0).unwrap().get_public_key());

    // create the operation
    let operation = Operation::new_verifiable(
        Operation {
            fee: Amount::zero(),
            expire_period: 10,
            op: OperationType::Transaction {
                recipient_address,
                amount: Amount::from_str("100").unwrap(),
            },
        },
        OperationSerializer::new(),
        &keypair,
    )
    .unwrap();
    // create the block containing the transaction operation
    universe.storage.store_operations(vec![operation.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &block_producer,
        Slot::new(1, 0),
        vec![operation],
        vec![],
        vec![],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());
    // set our block as a final block so the transaction is processed
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
    block_metadata.insert(
        block.id,
        ExecutionBlockMetadata {
            same_thread_parent_creator: Some(Address::from_public_key(&keypair.get_public_key())),
            storage: Some(universe.storage.clone()),
        },
    );
    universe.module_controller.update_blockclique_status(
        finalized_blocks,
        Default::default(),
        block_metadata.clone(),
    );
    std::thread::sleep(Duration::from_millis(10));
    // check recipient balance
    //TODO: replace when ledger will be mocked
    assert_eq!(
        universe
            .final_state
            .read()
            .ledger
            .get_balance(&recipient_address)
            .unwrap(),
        // Storage cost applied
        Amount::from_str("100")
            .unwrap()
            // Storage cost base
            .saturating_sub(LEDGER_ENTRY_BASE_COST)
    );
}

#[test]
fn roll_buy() {
    // setup the period duration
    let exec_cfg = ExecutionConfig {
        t0: MassaTime::from_millis(100),
        cursor_delay: MassaTime::from_millis(0),
        ..ExecutionConfig::default()
    };
    let block_producer = KeyPair::generate(0).unwrap();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
    let address = Address::from_public_key(&keypair.get_public_key());

    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(
        &mut foreign_controllers.selector_controller,
        &block_producer,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);
    // create the operation
    let operation = Operation::new_verifiable(
        Operation {
            fee: Amount::zero(),
            expire_period: 10,
            op: OperationType::RollBuy { roll_count: 10 },
        },
        OperationSerializer::new(),
        &keypair,
    )
    .unwrap();
    // create the block containing the roll buy operation
    universe.storage.store_operations(vec![operation.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &block_producer,
        Slot::new(1, 0),
        vec![operation],
        vec![],
        vec![],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());
    // set our block as a final block so the purchase is processed
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
    block_metadata.insert(
        block.id,
        ExecutionBlockMetadata {
            same_thread_parent_creator: Some(Address::from_public_key(&keypair.get_public_key())),
            storage: Some(universe.storage.clone()),
        },
    );
    universe.module_controller.update_blockclique_status(
        finalized_blocks,
        Default::default(),
        block_metadata.clone(),
    );
    std::thread::sleep(Duration::from_millis(100));
    // check roll count of the buyer address and its balance
    let sample_read = universe.final_state.read();
    assert_eq!(sample_read.pos_state.get_rolls_for(&address), 110);
    assert_eq!(
        sample_read.ledger.get_balance(&address).unwrap(),
        Amount::from_str("299_000").unwrap()
    );
}

#[test]
fn roll_sell() {
    // Try to sell 10 rolls (operation 1) then 1 rolls (operation 2)
    // Check for resulting roll count + resulting deferred credits

    // setup the period duration
    // turn off roll selling on missed block opportunities
    // otherwise balance will be credited with those sold roll (and we need to check the balance for
    // if the deferred credits are reimbursed
    let exec_cfg = ExecutionConfig {
        t0: MassaTime::from_millis(100),
        cursor_delay: MassaTime::from_millis(0),
        periods_per_cycle: 2,
        thread_count: 2,
        last_start_period: 2,
        max_miss_ratio: Ratio::new(1, 1),
        ..Default::default()
    };
    let block_producer = KeyPair::generate(0).unwrap();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
    let address = Address::from_public_key(&keypair.get_public_key());
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(
        &mut foreign_controllers.selector_controller,
        &block_producer,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());

    // get initial balance
    let balance_initial = universe.final_state.read().ledger.get_balance(&address).unwrap();

    // get initial roll count
    let roll_count_initial = universe.final_state.read().pos_state.get_rolls_for(&address);
    let roll_sell_1 = 10;
    let roll_sell_2 = 1;
    let roll_sell_3 = roll_count_initial.saturating_add(10);

    let initial_deferred_credits = Amount::from_str("100").unwrap();

    let mut batch = DBBatch::new();

    // set initial_deferred_credits that will be reimbursed at first block
    universe.final_state.write().pos_state.put_deferred_credits_entry(
        &Slot::new(1, 0),
        &address,
        &initial_deferred_credits,
        &mut batch,
    );

    universe
        .final_state
        .write()
        .db
        .write()
        .write_batch(batch, Default::default(), None);

    // create operation 1
    let operation1 = Operation::new_verifiable(
        Operation {
            fee: Amount::zero(),
            expire_period: 10,
            op: OperationType::RollSell {
                roll_count: roll_sell_1,
            },
        },
        OperationSerializer::new(),
        &keypair,
    )
    .unwrap();
    let operation2 = Operation::new_verifiable(
        Operation {
            fee: Amount::zero(),
            expire_period: 10,
            op: OperationType::RollSell {
                roll_count: roll_sell_2,
            },
        },
        OperationSerializer::new(),
        &keypair,
    )
    .unwrap();
    let operation3 = Operation::new_verifiable(
        Operation {
            fee: Amount::zero(),
            expire_period: 10,
            op: OperationType::RollSell {
                roll_count: roll_sell_3,
            },
        },
        OperationSerializer::new(),
        &keypair,
    )
    .unwrap();
    // create the block containing the roll buy operation
    universe.storage.store_operations(vec![
        operation1.clone(),
        operation2.clone(),
        operation3.clone(),
    ]);
    let block = ExecutionTestUniverse::create_block(
        &KeyPair::generate(0).unwrap(),
        Slot::new(3, 0),
        vec![operation1, operation2, operation3],
        vec![],
        vec![]
    );
    // store the block in storage
    universe.storage.store_block(block.clone());
    // set the block as final so the sell and credits are processed
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
    block_metadata.insert(
        block.id,
        ExecutionBlockMetadata {
            same_thread_parent_creator: Some(address),
            storage: Some(universe.storage.clone()),
        },
    );
    universe.module_controller.update_blockclique_status(
        finalized_blocks,
        Default::default(),
        block_metadata.clone(),
    );
    std::thread::sleep(Duration::from_millis(1000));

    // check roll count deferred credits and candidate balance of the seller address
    let sample_read = universe.final_state.read();
    let mut credits = PreHashMap::default();
    let roll_remaining = roll_count_initial - roll_sell_1 - roll_sell_2;
    let roll_sold = roll_sell_1 + roll_sell_2;
    credits.insert(
        address,
        exec_cfg.roll_price.checked_mul_u64(roll_sold).unwrap(),
    );

    assert_eq!(
        sample_read.pos_state.get_rolls_for(&address),
        roll_remaining
    );

    assert_eq!(
        sample_read
            .pos_state
            .get_deferred_credits_range(..=Slot::new(9, 1))
            .credits
            .get(&Slot::new(9, 1))
            .cloned()
            .unwrap_or_default(),
        credits
    );

    // Check that deferred credit are reimbursed
    let credits = PreHashMap::default();

    assert_eq!(
        sample_read
            .pos_state
            .get_deferred_credits_range(..=Slot::new(10, 1))
            .credits
            .get(&Slot::new(10, 1))
            .cloned()
            .unwrap_or_default(),
        credits
    );

    // Check that the initial deferred_credits are set to zero
    assert_eq!(
        sample_read
            .pos_state
            .get_deferred_credits_range(..=Slot::new(10, 1))
            .credits
            .get(&Slot::new(1, 0))
            .cloned()
            .unwrap_or_default(),
        credits
    );

    // Now check balance
    let balances = universe
        .module_controller
        .get_final_and_candidate_balance(&[address]);
    let candidate_balance = balances.get(0).unwrap().1.unwrap();

    assert_eq!(
        candidate_balance,
        exec_cfg
            .roll_price
            .checked_mul_u64(roll_sell_1 + roll_sell_2)
            .unwrap()
            .checked_add(balance_initial)
            .unwrap()
            .checked_add(initial_deferred_credits)
            .unwrap()
    );
}

#[test]
fn roll_slash() {
    // Try to sell 97 rolls (operation 1) then process a Denunciation (with config set to slash
    // 3 rolls)
    // Check for resulting roll & deferred credits & balance

    // setup the period duration
    // turn off roll selling on missed block opportunities
    // otherwise balance will be credited with those sold roll (and we need to check the balance for
    // if the deferred credits are reimbursed
    let exec_cfg = ExecutionConfig {
        t0: MassaTime::from_millis(100),
        cursor_delay: MassaTime::from_millis(0),
        periods_per_cycle: 2,
        thread_count: 2,
        last_start_period: 2,
        roll_count_to_slash_on_denunciation: 3, // Set to 3 to check if config is taken into account
        max_miss_ratio: Ratio::new(1, 1),
        ..Default::default()
    };
    let block_producer = KeyPair::generate(0).unwrap();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
    let address = Address::from_public_key(&keypair.get_public_key());

    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(&mut foreign_controllers.selector_controller, &keypair);
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());

    // get initial balance
    let balance_initial = universe
        .final_state
        .read()
        .ledger
        .get_balance(&address)
        .unwrap();

    // get initial roll count
    let roll_count_initial = universe
        .final_state
        .read()
        .pos_state
        .get_rolls_for(&address);
    let roll_to_sell = roll_count_initial
        .checked_sub(exec_cfg.roll_count_to_slash_on_denunciation)
        .unwrap();

    // create operation 1
    let operation1 = Operation::new_verifiable(
        Operation {
            fee: Amount::zero(),
            expire_period: 8,
            op: OperationType::RollSell {
                roll_count: roll_to_sell,
            },
        },
        OperationSerializer::new(),
        &keypair,
    )
    .unwrap();

    // create a denunciation
    let (_slot, _keypair, s_endorsement_1, s_endorsement_2, _) =
        gen_endorsements_for_denunciation(Some(Slot::new(3, 0)), Some(keypair.clone()));
    let denunciation = Denunciation::try_from((&s_endorsement_1, &s_endorsement_2)).unwrap();

    // create a denunciation (that will be ignored as it has been created at the last start period)
    let (_slot, _keypair, s_endorsement_1, s_endorsement_2, _) = gen_endorsements_for_denunciation(
        Some(Slot::new(exec_cfg.last_start_period, 4)),
        Some(keypair.clone()),
    );
    let denunciation_2 = Denunciation::try_from((&s_endorsement_1, &s_endorsement_2)).unwrap();

    // create the block containing the roll buy operation
    universe.storage.store_operations(vec![operation1.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &block_producer,
        Slot::new(3, 0),
        vec![operation1],
        vec![],
        vec![denunciation.clone(), denunciation, denunciation_2],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());
    // set the block as final so the sell and credits are processed
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
    block_metadata.insert(
        block.id,
        ExecutionBlockMetadata {
            same_thread_parent_creator: Some(Address::from_public_key(
                &block_producer.get_public_key(),
            )),
            storage: Some(universe.storage.clone()),
        },
    );
    universe.module_controller.update_blockclique_status(
        finalized_blocks,
        Default::default(),
        block_metadata.clone(),
    );
    std::thread::sleep(Duration::from_millis(1000));

    // check roll count deferred credits and candidate balance of the seller address
    let sample_read = universe.final_state.read();
    let mut credits = PreHashMap::default();
    let roll_sold = roll_to_sell;
    credits.insert(
        address,
        exec_cfg.roll_price.checked_mul_u64(roll_sold).unwrap(),
    );

    assert_eq!(sample_read.pos_state.get_rolls_for(&address), 0);

    // Check the remaining deferred credits
    let slot_limit = Slot::new(10, 0);
    let deferred_credits = sample_read
        .pos_state
        .get_deferred_credits_range(..=slot_limit)
        .credits;

    let (_slot, deferred_credit_amounts) = deferred_credits.last_key_value().unwrap();

    assert_eq!(
        *deferred_credit_amounts.get(&address).unwrap(),
        exec_cfg.roll_price.checked_mul_u64(roll_to_sell).unwrap()
    );

    // Now check balance
    let balances = universe
        .module_controller
        .get_final_and_candidate_balance(&[address]);
    let candidate_balance = balances.get(0).unwrap().1.unwrap();

    assert_eq!(
        candidate_balance,
        exec_cfg
            .roll_price
            .checked_mul_u64(roll_to_sell)
            .unwrap()
            .checked_add(balance_initial)
            .unwrap()
    );
}

#[test]
fn roll_slash_2() {
    // Try to sell all rolls (operation 1) then process a Denunciation (with config set to slash
    // 4 rolls)
    // Check for resulting roll & deferred credits & balance

    // setup the period duration
    // turn off roll selling on missed block opportunities
    // otherwise balance will be credited with those sold roll (and we need to check the balance for
    // if the deferred credits are reimbursed
    let exec_cfg = ExecutionConfig {
        t0: MassaTime::from_millis(100),
        cursor_delay: MassaTime::from_millis(0),
        periods_per_cycle: 2,
        thread_count: 2,
        last_start_period: 2,
        roll_count_to_slash_on_denunciation: 3, // Set to 3 to check if config is taken into account
        max_miss_ratio: Ratio::new(1, 1),
        ..Default::default()
    };
    let block_producer = KeyPair::generate(0).unwrap();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
    let address = Address::from_public_key(&keypair.get_public_key());

    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(&mut foreign_controllers.selector_controller, &keypair);
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());

    // get initial balance
    let balance_initial = universe
        .final_state
        .read()
        .ledger
        .get_balance(&address)
        .unwrap();

    // get initial roll count
    let roll_count_initial = universe
        .final_state
        .read()
        .pos_state
        .get_rolls_for(&address);
    // sell all rolls so we can check if slash will occur on deferred credits
    let roll_to_sell_1 = 1;
    let roll_to_sell_2 = roll_count_initial - 1;
    let roll_to_sell = roll_to_sell_1 + roll_to_sell_2;

    //
    let amount_def = exec_cfg
        .roll_price
        .checked_mul_u64(exec_cfg.roll_count_to_slash_on_denunciation)
        .unwrap();

    // create operation 1
    let operation1 = Operation::new_verifiable(
        Operation {
            fee: Amount::zero(),
            expire_period: 8,
            op: OperationType::RollSell {
                roll_count: roll_to_sell_1,
            },
        },
        OperationSerializer::new(),
        &keypair,
    )
    .unwrap();

    // create operation 2
    let operation2 = Operation::new_verifiable(
        Operation {
            fee: Amount::zero(),
            expire_period: 8,
            op: OperationType::RollSell {
                roll_count: roll_to_sell_2,
            },
        },
        OperationSerializer::new(),
        &keypair,
    )
    .unwrap();

    // create a denunciation
    let (_slot, _keypair, s_endorsement_1, s_endorsement_2, _) =
        gen_endorsements_for_denunciation(Some(Slot::new(3, 0)), Some(keypair.clone()));
    let denunciation = Denunciation::try_from((&s_endorsement_1, &s_endorsement_2)).unwrap();

    // create the block containing the roll buy operation
    universe
        .storage
        .store_operations(vec![operation1.clone(), operation2.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &block_producer,
        Slot::new(3, 0),
        vec![operation1, operation2],
        vec![],
        vec![denunciation.clone(), denunciation],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());
    // set the block as final so the sell and credits are processed
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
    block_metadata.insert(
        block.id,
        ExecutionBlockMetadata {
            same_thread_parent_creator: Some(Address::from_public_key(&keypair.get_public_key())),
            storage: Some(universe.storage.clone()),
        },
    );
    universe.module_controller.update_blockclique_status(
        finalized_blocks,
        Default::default(),
        block_metadata.clone(),
    );
    std::thread::sleep(Duration::from_millis(1000));

    // check roll count & deferred credits & candidate balance
    let sample_read = universe.final_state.read();
    let mut credits = PreHashMap::default();
    let roll_sold = roll_to_sell;
    credits.insert(
        address,
        exec_cfg.roll_price.checked_mul_u64(roll_sold).unwrap(),
    );

    assert_eq!(sample_read.pos_state.get_rolls_for(&address), 0);

    // Check the remaining deferred credits
    let slot_limit = Slot::new(10, 0);
    let deferred_credits = sample_read
        .pos_state
        .get_deferred_credits_range(..=slot_limit)
        .credits;

    let (_slot, deferred_credit_amounts) = deferred_credits.last_key_value().unwrap();

    assert_eq!(
        *deferred_credit_amounts.get(&address).unwrap(),
        exec_cfg
            .roll_price
            .checked_mul_u64(roll_to_sell)
            .unwrap()
            .checked_sub(amount_def)
            .unwrap()
    );

    // Now check balance
    let balances = universe
        .module_controller
        .get_final_and_candidate_balance(&[address]);
    let candidate_balance = balances.get(0).unwrap().1.unwrap();

    assert_eq!(
        candidate_balance,
        exec_cfg
            .roll_price
            .checked_mul_u64(roll_to_sell)
            .unwrap()
            .checked_sub(amount_def)
            .unwrap()
            .checked_add(balance_initial)
            .unwrap()
    );
}

#[test]
fn sc_execution_error() {
    // setup the period duration and the maximum gas for asynchronous messages execution
    let exec_cfg = ExecutionConfig {
        t0: MassaTime::from_millis(100),
        cursor_delay: MassaTime::from_millis(0),
        max_async_gas: 100_000,
        ..ExecutionConfig::default()
    };
    let block_producer = KeyPair::generate(0).unwrap();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();

    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(
        &mut foreign_controllers.selector_controller,
        &block_producer,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());
    // load bytecode
    // you can check the source code of the following wasm file in massa-unit-tests-src
    let bytecode = include_bytes!("./wasm/execution_error.wasm");
    // create the block containing the erroneous smart contract execution operation
    let operation =
        ExecutionTestUniverse::create_execute_sc_operation(&keypair, bytecode, BTreeMap::default())
            .unwrap();
    universe.storage.store_operations(vec![operation.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &keypair,
        Slot::new(1, 0),
        vec![operation],
        vec![],
        vec![],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());
    // set our block as a final block
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
    block_metadata.insert(
        block.id,
        ExecutionBlockMetadata {
            same_thread_parent_creator: Some(Address::from_public_key(&keypair.get_public_key())),
            storage: Some(universe.storage.clone()),
        },
    );
    universe.module_controller.update_blockclique_status(
        finalized_blocks,
        Default::default(),
        block_metadata.clone(),
    );
    std::thread::sleep(Duration::from_millis(100));

    // retrieve the event emitted by the execution error
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter {
            is_error: Some(true),
            ..Default::default()
        });
    // match the events
    assert!(!events.is_empty(), "2 events were expected");
    assert_eq!(events[0].data, "event generated before the sc failure");
    assert!(events[1].data.contains("massa_execution_error"));
    assert!(events[1]
        .data
        .contains("runtime error when executing operation"));
    assert!(events[1].data.contains("address parsing error"));
}

#[test]
fn sc_datastore() {
    // setup the period duration and the maximum gas for asynchronous messages execution
    let exec_cfg = ExecutionConfig {
        t0: MassaTime::from_millis(100),
        cursor_delay: MassaTime::from_millis(0),
        max_async_gas: 100_000,
        ..ExecutionConfig::default()
    };
    let block_producer = KeyPair::generate(0).unwrap();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();

    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(
        &mut foreign_controllers.selector_controller,
        &block_producer,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());
    // load bytecode
    // you can check the source code of the following wasm file in massa-unit-tests-src
    let bytecode = include_bytes!("./wasm/datastore.wasm");
    let datastore = BTreeMap::from([(vec![65, 66], vec![255]), (vec![9], vec![10, 11])]);

    // create the block containing the erroneous smart contract execution operation
    let operation =
        ExecutionTestUniverse::create_execute_sc_operation(&keypair, bytecode, datastore).unwrap();
    universe.storage.store_operations(vec![operation.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &keypair,
        Slot::new(1, 0),
        vec![operation],
        vec![],
        vec![],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());
    // set our block as a final block
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
    block_metadata.insert(
        block.id,
        ExecutionBlockMetadata {
            same_thread_parent_creator: Some(Address::from_public_key(&keypair.get_public_key())),
            storage: Some(universe.storage.clone()),
        },
    );
    universe.module_controller.update_blockclique_status(
        finalized_blocks,
        Some(Default::default()),
        block_metadata,
    );
    std::thread::sleep(Duration::from_millis(100));

    // retrieve the event emitted by the execution error
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter::default());

    // match the events
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].data, "keys: 9,65,66");
    assert_eq!(events[1].data, "has_key_1: true - has_key_2: false");
    assert_eq!(events[2].data, "data key 1: 255 - data key 3: 10,11");
}

#[test]
fn set_bytecode_error() {
    // setup the period duration
    let exec_cfg = ExecutionConfig {
        t0: MassaTime::from_millis(100),
        cursor_delay: MassaTime::from_millis(0),
        max_async_gas: 100_000,
        ..ExecutionConfig::default()
    };
    let block_producer = KeyPair::generate(0).unwrap();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();

    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(
        &mut foreign_controllers.selector_controller,
        &block_producer,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());
    // load bytecodes
    // you can check the source code of the following wasm file in massa-unit-tests-src
    let bytecode = include_bytes!("./wasm/set_bytecode_fail.wasm");
    let datastore_bytecode = include_bytes!("./wasm/smart-contract.wasm").to_vec();
    let mut datastore = BTreeMap::new();
    datastore.insert(b"smart-contract".to_vec(), datastore_bytecode);

    // create the block containing the erroneous smart contract execution operation
    let operation =
        ExecutionTestUniverse::create_execute_sc_operation(&keypair, bytecode, datastore).unwrap();
    universe.storage.store_operations(vec![operation.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &keypair,
        Slot::new(1, 0),
        vec![operation],
        vec![],
        vec![],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());
    // set our block as a final block
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
    block_metadata.insert(
        block.id,
        ExecutionBlockMetadata {
            same_thread_parent_creator: Some(Address::from_public_key(&keypair.get_public_key())),
            storage: Some(universe.storage.clone()),
        },
    );
    universe.module_controller.update_blockclique_status(
        finalized_blocks,
        Default::default(),
        block_metadata.clone(),
    );
    std::thread::sleep(Duration::from_millis(10));

    // retrieve the event emitted by the execution error
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter::default());
    // match the events
    assert!(!events.is_empty(), "One event was expected");
    assert!(events[0].data.contains("massa_execution_error"));
    assert!(events[0]
        .data
        .contains("runtime error when executing operation"));
    assert!(events[0].data.contains("can't set the bytecode of address"));
}

#[test]
fn datastore_manipulations() {
    // setup the period duration
    let exec_cfg = ExecutionConfig {
        t0: MassaTime::from_millis(100),
        cursor_delay: MassaTime::from_millis(0),
        ..ExecutionConfig::default()
    };
    let block_producer = KeyPair::generate(0).unwrap();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();

    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(
        &mut foreign_controllers.selector_controller,
        &block_producer,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());
    // load bytecode
    // you can check the source code of the following wasm file in massa-unit-tests-src
    let bytecode = include_bytes!("./wasm/datastore_manipulations.wasm");
    // create the block containing the erroneous smart contract execution operation
    let operation =
        ExecutionTestUniverse::create_execute_sc_operation(&keypair, bytecode, BTreeMap::default())
            .unwrap();
    universe.storage.store_operations(vec![operation.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &block_producer,
        Slot::new(1, 0),
        vec![operation.clone()],
        vec![],
        vec![],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());
    // set our block as a final block
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let block_store = vec![(
        block.id,
        ExecutionBlockMetadata {
            storage: Some(universe.storage.clone()),
            same_thread_parent_creator: Some(Address::from_public_key(&keypair.get_public_key())),
        },
    )]
    .into_iter()
    .collect();
    universe.module_controller.update_blockclique_status(
        finalized_blocks,
        Default::default(),
        block_store,
    );
    std::thread::sleep(
        exec_cfg
            .t0
            .saturating_add(MassaTime::from_millis(50))
            .into(),
    );

    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter::default());
    // match the events
    println!("{:?}", events);
    assert_eq!(events.len(), 4, "Got {} events, expected 4", events.len());
    let key_a: Vec<u8> = [1, 0, 4, 255].to_vec();
    let key_a_str: String = key_a
        .iter()
        .map(|b| format!("{}", b))
        .collect::<Vec<String>>()
        .join(",");

    let key_b: Vec<u8> = [2, 0, 254, 255].to_vec();
    let key_b_str: String = key_b
        .iter()
        .map(|b| format!("{}", b))
        .collect::<Vec<String>>()
        .join(",");

    assert!(
        events[0].data.contains(&format!("keys: {}", key_a_str)),
        "{:?}",
        events[0].data
    );
    assert!(
        events[1].data.contains(&format!("keys2: {}", key_a_str)),
        "{:?}",
        events[1].data
    );
    assert!(
        events[2].data.contains(&format!("keys_f: {}", key_b_str)),
        "{:?}",
        events[2].data
    );
    assert!(
        events[3].data.contains(&format!("keys2_f: {}", key_a_str)),
        "{:?}",
        events[3].data
    );

    // Length of the key and value left in the datastore. See sources for more context.
    let key_len = (key_a.len() + key_b.len()) as u64;
    let value_len = ([21, 0, 49].len() + [5, 12, 241].len()) as u64;

    let addr = Address::from_public_key(&keypair.get_public_key());
    let amount = universe
        .final_state
        .read()
        .ledger
        .get_balance(&addr)
        .unwrap();
    assert_eq!(
        amount,
        Amount::from_str("300000")
            .unwrap()
            // Gas fee
            .saturating_sub(Amount::const_init(10, 0))
            // Storage cost base
            .saturating_sub(
                exec_cfg
                    .storage_costs_constants
                    .ledger_cost_per_byte
                    .saturating_mul_u64(2 * LEDGER_ENTRY_DATASTORE_BASE_SIZE as u64)
            )
            // Storage cost key
            .saturating_sub(
                exec_cfg
                    .storage_costs_constants
                    .ledger_cost_per_byte
                    .saturating_mul_u64(key_len)
            )
            // Storage cost value
            .saturating_sub(
                exec_cfg
                    .storage_costs_constants
                    .ledger_cost_per_byte
                    .saturating_mul_u64(value_len)
            )
    );

    universe
        .module_controller
        .query_state(ExecutionQueryRequest {
            requests: vec![
                ExecutionQueryRequestItem::AddressExistsCandidate(addr.clone()),
                ExecutionQueryRequestItem::AddressExistsFinal(addr.clone()),
                ExecutionQueryRequestItem::AddressBalanceCandidate(addr.clone()),
                ExecutionQueryRequestItem::AddressBalanceFinal(addr.clone()),
                ExecutionQueryRequestItem::AddressBytecodeCandidate(addr.clone()),
                ExecutionQueryRequestItem::AddressBytecodeFinal(addr.clone()),
                ExecutionQueryRequestItem::AddressDatastoreKeysCandidate {
                    addr: addr.clone(),
                    prefix: vec![],
                },
                ExecutionQueryRequestItem::AddressDatastoreKeysFinal {
                    addr: addr.clone(),
                    prefix: vec![],
                },
                ExecutionQueryRequestItem::AddressDatastoreValueCandidate {
                    addr: addr.clone(),
                    key: key_a.clone(),
                },
                ExecutionQueryRequestItem::AddressDatastoreValueFinal {
                    addr: addr.clone(),
                    key: key_a.clone(),
                },
                ExecutionQueryRequestItem::OpExecutionStatusCandidate(operation.id),
                ExecutionQueryRequestItem::OpExecutionStatusFinal(operation.id),
                ExecutionQueryRequestItem::AddressRollsCandidate(addr.clone()),
                ExecutionQueryRequestItem::AddressRollsFinal(addr.clone()),
                ExecutionQueryRequestItem::AddressDeferredCreditsCandidate(addr.clone()),
                ExecutionQueryRequestItem::AddressDeferredCreditsFinal(addr.clone()),
                ExecutionQueryRequestItem::CycleInfos {
                    cycle: 0,
                    restrict_to_addresses: None,
                },
                ExecutionQueryRequestItem::Events(EventFilter::default()),
            ],
        });
    universe
        .module_controller
        .get_addresses_infos(&[addr.clone()]);
}

/// This test checks causes a history rewrite in slot sequencing and ensures that emitted events match
#[test]
fn events_from_switching_blockclique() {
    // setup the period duration and the maximum gas for asynchronous messages execution
    let exec_cfg = ExecutionConfig {
        t0: MassaTime::from_millis(100),
        cursor_delay: MassaTime::from_millis(0),
        ..ExecutionConfig::default()
    };
    let block_producer = KeyPair::generate(0).unwrap();

    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(
        &mut foreign_controllers.selector_controller,
        &block_producer,
    );

    let universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());

    let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
    let mut blockclique_blocks: HashMap<Slot, BlockId> = HashMap::new();

    // create blockclique block at slot (1,1)
    {
        let blockclique_block_slot = Slot::new(1, 1);
        let keypair = KeyPair::from_str(TEST_SK_2).unwrap();
        let event_test_data = include_bytes!("./wasm/event_test.wasm");
        let operation = ExecutionTestUniverse::create_execute_sc_operation(
            &keypair,
            event_test_data,
            BTreeMap::default(),
        )
        .unwrap();
        let blockclique_block = ExecutionTestUniverse::create_block(
            &block_producer,
            blockclique_block_slot,
            vec![operation.clone()],
            vec![],
            vec![],
        );
        blockclique_blocks.insert(blockclique_block_slot, blockclique_block.id);
        let mut blockclique_block_storage = universe.storage.clone_without_refs();
        blockclique_block_storage.store_block(blockclique_block.clone());
        blockclique_block_storage.store_operations(vec![operation]);
        block_metadata.insert(
            blockclique_block.id,
            ExecutionBlockMetadata {
                storage: Some(blockclique_block_storage),
                same_thread_parent_creator: Some(Address::from_public_key(
                    &keypair.get_public_key(),
                )),
            },
        );
    }
    // notify execution about blockclique change
    universe.module_controller.update_blockclique_status(
        Default::default(),
        Some(blockclique_blocks.clone()),
        block_metadata.clone(),
    );
    std::thread::sleep(Duration::from_millis(1000));
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter::default());
    assert_eq!(events.len(), 1, "wrong event count");
    assert_eq!(events[0].context.slot, Slot::new(1, 1), "Wrong event slot");

    // create blockclique block at slot (1,0)
    {
        let blockclique_block_slot = Slot::new(1, 0);
        let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
        let event_test_data = include_bytes!("./wasm/event_test.wasm");
        let operation = ExecutionTestUniverse::create_execute_sc_operation(
            &keypair,
            event_test_data,
            BTreeMap::default(),
        )
        .unwrap();
        let blockclique_block = ExecutionTestUniverse::create_block(
            &keypair,
            blockclique_block_slot,
            vec![operation.clone()],
            vec![],
            vec![],
        );
        blockclique_blocks.insert(blockclique_block_slot, blockclique_block.id);
        let mut blockclique_block_storage = universe.storage.clone_without_refs();
        blockclique_block_storage.store_block(blockclique_block.clone());
        blockclique_block_storage.store_operations(vec![operation]);
        block_metadata.insert(
            blockclique_block.id,
            ExecutionBlockMetadata {
                storage: Some(blockclique_block_storage),
                same_thread_parent_creator: Some(Address::from_public_key(
                    &keypair.get_public_key(),
                )),
            },
        );
    }
    // notify execution about blockclique change
    universe.module_controller.update_blockclique_status(
        Default::default(),
        Some(blockclique_blocks.clone()),
        block_metadata.clone(),
    );
    std::thread::sleep(Duration::from_millis(1000));
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter::default());
    assert_eq!(events.len(), 2, "wrong event count");
    assert_eq!(events[0].context.slot, Slot::new(1, 0), "Wrong event slot");
    assert_eq!(events[1].context.slot, Slot::new(1, 1), "Wrong event slot");
}

#[test]
fn not_enough_compilation_gas() {
    // setup the period duration
    let exec_cfg = ExecutionConfig {
        t0: MassaTime::from_millis(100),
        cursor_delay: MassaTime::from_millis(0),
        ..ExecutionConfig::default()
    };
    let block_producer = KeyPair::generate(0).unwrap();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();

    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(
        &mut foreign_controllers.selector_controller,
        &block_producer,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());

    // load bytecode
    // you can check the source code of the following wasm file in massa-unit-tests-src
    let bytecode = include_bytes!("./wasm/datastore_manipulations.wasm");
    // create the block containing the operation
    let operation = Operation::new_verifiable(
        Operation {
            fee: Amount::const_init(10, 0),
            expire_period: 10,
            op: OperationType::ExecuteSC {
                max_coins: Amount::const_init(0, 0),
                data: bytecode.to_vec(),
                max_gas: 0,
                datastore: BTreeMap::default(),
            },
        },
        OperationSerializer::new(),
        &keypair,
    )
    .unwrap();
    universe.storage.store_operations(vec![operation.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &keypair,
        Slot::new(1, 0),
        vec![operation],
        vec![],
        vec![],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());
    // set our block as a final block
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let block_store = vec![(
        block.id,
        ExecutionBlockMetadata {
            storage: Some(universe.storage.clone()),
            same_thread_parent_creator: Some(Address::from_public_key(&keypair.get_public_key())),
        },
    )]
    .into_iter()
    .collect();

    // update blockclique
    universe.module_controller.update_blockclique_status(
        finalized_blocks,
        Default::default(),
        block_store,
    );
    std::thread::sleep(Duration::from_millis(100));

    // assert events
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter::default());
    assert!(events[0]
        .data
        .contains("not enough gas to pay for singlepass compilation"));
}

#[test]
fn sc_builtins() {
    // setup the period duration
    let exec_cfg = ExecutionConfig {
        t0: MassaTime::from_millis(100),
        cursor_delay: MassaTime::from_millis(0),
        ..ExecutionConfig::default()
    };
    let block_producer = KeyPair::generate(0).unwrap();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();

    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(
        &mut foreign_controllers.selector_controller,
        &block_producer,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());
    // load bytecode
    // you can check the source code of the following wasm file in massa-unit-tests-src
    let bytecode = include_bytes!("./wasm/use_builtins.wasm");
    // create the block containing the erroneous smart contract execution operation
    let operation =
        ExecutionTestUniverse::create_execute_sc_operation(&keypair, bytecode, BTreeMap::default())
            .unwrap();
    universe.storage.store_operations(vec![operation.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &block_producer,
        Slot::new(1, 0),
        vec![operation],
        vec![],
        vec![],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());
    // set our block as a final block
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
    block_metadata.insert(
        block.id,
        ExecutionBlockMetadata {
            same_thread_parent_creator: Some(Address::from_public_key(&keypair.get_public_key())),
            storage: Some(universe.storage.clone()),
        },
    );
    universe.module_controller.update_blockclique_status(
        finalized_blocks,
        Default::default(),
        block_metadata.clone(),
    );
    std::thread::sleep(Duration::from_millis(10));

    // retrieve the event emitted by the execution error
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter::default());
    // match the events
    assert!(!events.is_empty(), "One event was expected");
    assert!(events[0].data.contains("massa_execution_error"));
    assert!(events[0]
        .data
        .contains("runtime error when executing operation"));
    dbg!(events[0].data.clone());
    assert!(events[0]
        .data
        .contains("abort with date and rnd at use_builtins.ts:0 col: 0"));

    assert_eq!(
        universe
            .final_state
            .read()
            .ledger
            .get_balance(&Address::from_public_key(&keypair.get_public_key()))
            .unwrap(),
        Amount::from_str("299990").unwrap()
    );
}

#[test]
fn validate_address() {
    // setup the period duration
    let exec_cfg = ExecutionConfig {
        t0: MassaTime::from_millis(100),
        cursor_delay: MassaTime::from_millis(0),
        ..ExecutionConfig::default()
    };
    let block_producer = KeyPair::generate(0).unwrap();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();

    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(
        &mut foreign_controllers.selector_controller,
        &block_producer,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());

    // load bytecode
    // you can check the source code of the following wasm file in massa-unit-tests-src
    let bytecode = include_bytes!("./wasm/validate_address.wasm");

    // create the block containing the erroneous smart contract execution operation
    let operation =
        ExecutionTestUniverse::create_execute_sc_operation(&keypair, bytecode, BTreeMap::default())
            .unwrap();
    universe.storage.store_operations(vec![operation.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &block_producer,
        Slot::new(1, 0),
        vec![operation],
        vec![],
        vec![],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());

    // set our block as a final block
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let block_store = vec![(
        block.id,
        ExecutionBlockMetadata {
            storage: Some(universe.storage.clone()),
            same_thread_parent_creator: Some(Address::from_public_key(&keypair.get_public_key())),
        },
    )]
    .into_iter()
    .collect();
    universe.module_controller.update_blockclique_status(
        finalized_blocks,
        Default::default(),
        block_store,
    );
    std::thread::sleep(
        exec_cfg
            .t0
            .saturating_add(MassaTime::from_millis(50))
            .into(),
    );

    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter::default());
    // match the events
    assert_eq!(events.len(), 2);
    assert!(
        events[0].data.ends_with("true"),
        "Expected 'true': {:?}",
        events[0].data
    );
    assert!(
        events[1].data.ends_with("false"),
        "Expected 'false': {:?}",
        events[1].data
    );

    assert_eq!(
        universe
            .final_state
            .read()
            .ledger
            .get_balance(&Address::from_public_key(&keypair.get_public_key()))
            .unwrap(),
        Amount::from_str("300000")
            .unwrap()
            // Gas fee
            .saturating_sub(Amount::const_init(10, 0))
    );
}

#[test]
fn test_rewards() {
    // setup the period duration
    let exec_cfg = ExecutionConfig {
        t0: MassaTime::from_millis(100),
        cursor_delay: MassaTime::from_millis(0),
        ..ExecutionConfig::default()
    };
    let block_producer = KeyPair::generate(0).unwrap();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(
        &mut foreign_controllers.selector_controller,
        &block_producer,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());

    // First block
    let block = ExecutionTestUniverse::create_block(
        &block_producer,
        Slot::new(1, 0),
        vec![],
        vec![
            ExecutionTestUniverse::create_endorsement(&block_producer, Slot::new(1, 0));
            ENDORSEMENT_COUNT as usize
        ],
        vec![],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());

    // set our block as a final block
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let block_store = vec![(
        block.id,
        ExecutionBlockMetadata {
            storage: Some(universe.storage.clone()),
            same_thread_parent_creator: Some(Address::from_public_key(
                &block_producer.get_public_key(),
            )),
        },
    )]
    .into_iter()
    .collect();
    universe.module_controller.update_blockclique_status(
        finalized_blocks,
        Default::default(),
        block_store,
    );
    std::thread::sleep(
        exec_cfg
            .t0
            .saturating_add(MassaTime::from_millis(50))
            .into(),
    );
    let (_, candidate_balance) = universe
        .module_controller
        .get_final_and_candidate_balance(&[Address::from_public_key(
            &block_producer.get_public_key(),
        )])[0];
    let first_block_reward = exec_cfg.block_reward.saturating_sub(LEDGER_ENTRY_BASE_COST);
    println!("{:#?}", candidate_balance);
    assert_eq!(candidate_balance.unwrap(), first_block_reward);

    // Second block
    let block = ExecutionTestUniverse::create_block(
        &block_producer,
        Slot::new(2, 0),
        vec![],
        vec![
            ExecutionTestUniverse::create_endorsement(&block_producer, Slot::new(2, 0));
            ENDORSEMENT_COUNT as usize
        ],
        vec![],
    );
    // store the block in storage
    universe.storage.store_block(block.clone());

    // set our block as a final block
    let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    finalized_blocks.insert(block.content.header.content.slot, block.id);
    let block_store = vec![(
        block.id,
        ExecutionBlockMetadata {
            storage: Some(universe.storage.clone()),
            same_thread_parent_creator: Some(Address::from_public_key(
                &block_producer.get_public_key(),
            )),
        },
    )]
    .into_iter()
    .collect();
    universe.module_controller.update_blockclique_status(
        finalized_blocks,
        Default::default(),
        block_store,
    );
    std::thread::sleep(
        exec_cfg
            .t0
            .saturating_add(MassaTime::from_millis(50))
            .into(),
    );
    let (_, candidate_balance) = universe
        .module_controller
        .get_final_and_candidate_balance(&[Address::from_public_key(
            &block_producer.get_public_key(),
        )])[0];
    assert_eq!(
        candidate_balance.unwrap(),
        first_block_reward.saturating_add(exec_cfg.block_reward)
    );
}
