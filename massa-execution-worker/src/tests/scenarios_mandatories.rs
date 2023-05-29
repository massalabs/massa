// Copyright (c) 2022 MASSA LABS <info@massa.net>
#[cfg(test)]
mod tests {
    use crate::start_execution_worker;
    use crate::tests::mock::{
        create_block, get_initials_vesting, get_random_address_full, get_sample_state,
    };
    use massa_execution_exports::{
        ExecutionChannels, ExecutionConfig, ExecutionController, ExecutionError,
        ReadOnlyExecutionRequest, ReadOnlyExecutionTarget,
    };
    use massa_models::config::{
        LEDGER_ENTRY_BASE_SIZE, LEDGER_ENTRY_DATASTORE_BASE_SIZE, MIP_STORE_STATS_BLOCK_CONSIDERED,
        MIP_STORE_STATS_COUNTERS_MAX,
    };
    use massa_models::prehash::PreHashMap;
    use massa_models::test_exports::gen_endorsements_for_denunciation;
    use massa_models::{address::Address, amount::Amount, slot::Slot};
    use massa_models::{
        block_id::BlockId,
        datastore::Datastore,
        denunciation::Denunciation,
        execution::EventFilter,
        operation::{Operation, OperationSerializer, OperationType, SecureShareOperation},
        secure_share::SecureShareContent,
    };
    use massa_signature::KeyPair;
    use massa_storage::Storage;
    use massa_time::MassaTime;
    use massa_versioning::versioning::{MipStatsConfig, MipStore};
    use num::rational::Ratio;
    use serial_test::serial;
    use std::{
        cmp::Reverse, collections::BTreeMap, collections::HashMap, str::FromStr, time::Duration,
    };
    use tokio::sync::broadcast;

    const TEST_SK_1: &str = "S18r2i8oJJyhF7Kprx98zwxAc3W4szf7RKuVMX6JydZz8zSxHeC";
    const TEST_SK_2: &str = "S1FpYC4ugG9ivZZbLVrTwWtF9diSRiAwwrVX5Gx1ANSRLfouUjq";
    const TEST_SK_3: &str = "S1LgXhWLEgAgCX3nm6y8PVPzpybmsYpi6yg6ZySwu5Z4ERnD7Bu";

    #[test]
    #[serial]
    fn test_execution_shutdown() {
        let vesting = get_initials_vesting(false);
        let config = ExecutionConfig {
            initial_vesting_path: vesting.path().to_path_buf(),
            ..ExecutionConfig::default()
        };
        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        let (sample_state, _keep_file, _keep_dir) = get_sample_state(0).unwrap();
        let (mut manager, _controller) = start_execution_worker(
            config,
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        manager.stop();
    }

    #[test]
    #[serial]
    fn test_sending_command() {
        let vesting = get_initials_vesting(false);
        let config = ExecutionConfig {
            initial_vesting_path: vesting.path().to_path_buf(),
            ..ExecutionConfig::default()
        };
        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        let (sample_state, _keep_file, _keep_dir) = get_sample_state(0).unwrap();
        let (mut manager, controller) = start_execution_worker(
            config,
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        controller.update_blockclique_status(
            Default::default(),
            Default::default(),
            Default::default(),
        );
        manager.stop();
    }

    #[test]
    #[serial]
    fn test_readonly_execution() {
        let vesting = get_initials_vesting(false);
        // setup the period duration
        let exec_cfg = ExecutionConfig {
            t0: MassaTime::from_millis(100),
            cursor_delay: MassaTime::from_millis(0),
            initial_vesting_path: vesting.path().to_path_buf(),
            ..ExecutionConfig::default()
        };
        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();
        // get a sample final state
        let (sample_state, _keep_file, _keep_dir) = get_sample_state(0).unwrap();
        // init the storage
        let storage = Storage::create_root();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        // start the execution worker
        let (mut manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());
        std::thread::sleep(Duration::from_millis(1000));

        let mut res = controller
            .execute_readonly_request(ReadOnlyExecutionRequest {
                max_gas: 1_000_000,
                call_stack: vec![],
                target: ReadOnlyExecutionTarget::BytecodeExecution(
                    include_bytes!("./wasm/event_test.wasm").to_vec(),
                ),
                is_final: true,
            })
            .expect("readonly execution failed");
        assert_eq!(res.out.slot, Slot::new(1, 0));
        assert!(res.gas_cost > 0);
        assert_eq!(res.out.events.take().len(), 1, "wrong number of events");

        let res = controller
            .execute_readonly_request(ReadOnlyExecutionRequest {
                max_gas: 1_000_000,
                call_stack: vec![],
                target: ReadOnlyExecutionTarget::BytecodeExecution(
                    include_bytes!("./wasm/event_test.wasm").to_vec(),
                ),
                is_final: false,
            })
            .expect("readonly execution failed");
        assert!(res.out.slot.period > 8);

        manager.stop();
    }

    /// Feeds the execution worker with genesis blocks to start it
    fn init_execution_worker(
        config: &ExecutionConfig,
        storage: &Storage,
        execution_controller: Box<dyn ExecutionController>,
    ) {
        let genesis_keypair = KeyPair::generate(0).unwrap();
        let mut finalized_blocks: HashMap<Slot, BlockId> = HashMap::new();
        let mut block_storage: PreHashMap<BlockId, Storage> = PreHashMap::default();
        for thread in 0..config.thread_count {
            let slot = Slot::new(0, thread);
            let final_block = create_block(genesis_keypair.clone(), vec![], vec![], slot).unwrap();
            finalized_blocks.insert(slot, final_block.id);
            let mut final_block_storage = storage.clone_without_refs();
            final_block_storage.store_block(final_block.clone());
            block_storage.insert(final_block.id, final_block_storage);
        }
        execution_controller.update_blockclique_status(
            finalized_blocks,
            Some(Default::default()),
            block_storage,
        );
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
        let vesting = get_initials_vesting(false);
        // setup the period duration
        let exec_cfg = ExecutionConfig {
            t0: MassaTime::from_millis(100),
            cursor_delay: MassaTime::from_millis(0),
            initial_vesting_path: vesting.path().to_path_buf(),
            ..ExecutionConfig::default()
        };
        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();
        // get a sample final state
        let (sample_state, _keep_file, _keep_dir) = get_sample_state(0).unwrap();
        // init the storage
        let mut storage = Storage::create_root();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        // start the execution worker
        let (mut manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());

        // get random keypair
        let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
        // load bytecodes
        // you can check the source code of the following wasm file in massa-unit-tests-src
        let bytecode = include_bytes!("./wasm/nested_call.wasm");
        let datastore_bytecode = include_bytes!("./wasm/test.wasm").to_vec();
        let mut datastore = BTreeMap::new();
        datastore.insert(b"smart-contract".to_vec(), datastore_bytecode);

        // create the block containing the smart contract execution operation
        let operation = create_execute_sc_operation(&keypair, bytecode, datastore).unwrap();
        storage.store_operations(vec![operation.clone()]);
        let block = create_block(
            KeyPair::generate(0).unwrap(),
            vec![operation],
            vec![],
            Slot::new(1, 0),
        )
        .unwrap();
        // store the block in storage
        storage.store_block(block.clone());

        // set our block as a final block so the message is sent
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        block_storage.insert(block.id, storage.clone());
        controller.update_blockclique_status(
            finalized_blocks.clone(),
            Default::default(),
            block_storage.clone(),
        );

        std::thread::sleep(Duration::from_millis(100));

        // length of the sub contract test.wasm
        // let bytecode_sub_contract_len = 4374;

        // let balance = sample_state
        //     .read()
        //     .ledger
        //     .get_balance(&Address::from_public_key(&keypair.get_public_key()))
        //     .unwrap();

        // let exec_cost = exec_cfg
        //     .storage_costs_constants
        //     .ledger_cost_per_byte
        //     .saturating_mul_u64(bytecode_sub_contract_len);

        // let balance_expected = Amount::from_str("300000")
        //     .unwrap()
        //     // Gas fee
        //     .saturating_sub(Amount::from_str("10").unwrap())
        //     // Storage cost base
        //     .saturating_sub(exec_cfg.storage_costs_constants.ledger_entry_base_cost)
        //     // Storage cost bytecode
        //     .saturating_sub(exec_cost);

        // assert_eq!(balance, balance_expected);
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
            Amount::from_str("0").unwrap(),
            Address::from_str(&address).unwrap(),
            String::from("test"),
            address.as_bytes().to_vec(),
        )
        .unwrap();
        // Init new storage for this block
        let mut storage = Storage::create_root();
        storage.store_operations(vec![operation.clone()]);
        let block = create_block(
            KeyPair::generate(0).unwrap(),
            vec![operation],
            vec![],
            Slot::new(2, 0),
        )
        .unwrap();
        // store the block in storage
        storage.store_block(block.clone());
        // set our block as a final block so the message is sent
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        block_storage.insert(block.id, storage.clone());
        controller.update_blockclique_status(
            finalized_blocks,
            Default::default(),
            block_storage.clone(),
        );
        std::thread::sleep(Duration::from_millis(100));
        // Get the events that give us the gas usage (refer to source in ts) without fetching the first slot because it emit a event with an address.
        let events = controller.get_filtered_sc_output_event(EventFilter {
            start: Some(Slot::new(2, 0)),
            ..Default::default()
        });
        assert!(events.len() > 0);
        // Check that we always subtract gas through the execution (even in sub calls)
        assert!(
            events.is_sorted_by_key(|event| Reverse(event.data.parse::<u64>().unwrap())),
            "Gas is not going down through the execution."
        );
        // stop the execution controller
        manager.stop();
    }

    /// Test the ABI get call coins
    ///
    /// Deploy an SC with a method `test` that generate an event saying how many coins he received
    /// Calling the SC in a second time
    #[test]
    #[serial]
    fn test_get_call_coins() {
        let vesting = get_initials_vesting(false);
        // setup the period duration
        let exec_cfg = ExecutionConfig {
            t0: MassaTime::from_millis(100),
            cursor_delay: MassaTime::from_millis(0),
            initial_vesting_path: vesting.path().to_path_buf(),
            ..ExecutionConfig::default()
        };
        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();
        // get a sample final state
        let (sample_state, _keep_file, _keep_dir) = get_sample_state(0).unwrap();
        // init the storage
        let mut storage = Storage::create_root();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        // start the execution worker
        let (mut manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());

        // get random keypair
        let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
        // load bytecodes
        // you can check the source code of the following wasm file in massa-unit-tests-src
        let bytecode = include_bytes!("./wasm/get_call_coins_main.wasm");
        let datastore_bytecode = include_bytes!("./wasm/get_call_coins_test.wasm").to_vec();
        let mut datastore = BTreeMap::new();
        datastore.insert(b"smart-contract".to_vec(), datastore_bytecode);

        // create the block containing the smart contract execution operation
        let operation = create_execute_sc_operation(&keypair, bytecode, datastore).unwrap();
        storage.store_operations(vec![operation.clone()]);
        let block = create_block(
            KeyPair::generate(0).unwrap(),
            vec![operation],
            vec![],
            Slot::new(1, 0),
        )
        .unwrap();
        // store the block in storage
        storage.store_block(block.clone());

        // set our block as a final block so the message is sent
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        block_storage.insert(block.id, storage.clone());
        controller.update_blockclique_status(
            finalized_blocks.clone(),
            Default::default(),
            block_storage.clone(),
        );

        std::thread::sleep(Duration::from_millis(100));

        // assert_eq!(balance, balance_expected);
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
        let coins_sent = Amount::from_str("10").unwrap();
        let operation = create_call_sc_operation(
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
        let mut storage = Storage::create_root();
        storage.store_operations(vec![operation.clone()]);
        let block = create_block(
            KeyPair::generate(0).unwrap(),
            vec![operation],
            vec![],
            Slot::new(2, 0),
        )
        .unwrap();
        // store the block in storage
        storage.store_block(block.clone());
        // set our block as a final block so the message is sent
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        block_storage.insert(block.id, storage.clone());
        controller.update_blockclique_status(
            finalized_blocks,
            Default::default(),
            block_storage.clone(),
        );
        std::thread::sleep(Duration::from_millis(100));
        // Get the events that give us the gas usage (refer to source in ts) without fetching the first slot because it emit a event with an address.
        let events = controller.get_filtered_sc_output_event(EventFilter {
            start: Some(Slot::new(2, 0)),
            ..Default::default()
        });
        println!("events {:#?}", events);
        assert!(events[0].data.contains(&format!(
            "tokens sent to the SC during the call : {}",
            coins_sent.to_raw()
        )));

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
        let vesting = get_initials_vesting(false);
        // setup the period duration and the maximum gas for asynchronous messages execution
        let exec_cfg = ExecutionConfig {
            t0: MassaTime::from_millis(100),
            cursor_delay: MassaTime::from_millis(0),
            max_async_gas: 100_000,
            initial_vesting_path: vesting.path().to_path_buf(),
            ..ExecutionConfig::default()
        };
        // get a sample final state
        let (sample_state, _keep_file, _keep_dir) = get_sample_state(0).unwrap();

        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        // init the storage
        let mut storage = Storage::create_root();
        // start the execution worker
        let (mut manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());
        // keypair associated to thread 0
        let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
        // load bytecodes
        // you can check the source code of the following wasm file in massa-unit-tests-src
        let bytecode = include_bytes!("./wasm/send_message.wasm");
        let datastore_bytecode = include_bytes!("./wasm/receive_message.wasm").to_vec();
        let mut datastore = BTreeMap::new();
        datastore.insert(b"smart-contract".to_vec(), datastore_bytecode);

        // create the block contaning the smart contract execution operation
        let operation = create_execute_sc_operation(&keypair, bytecode, datastore).unwrap();
        storage.store_operations(vec![operation.clone()]);
        let block = create_block(
            KeyPair::generate(0).unwrap(),
            vec![operation],
            vec![],
            Slot::new(1, 0),
        )
        .unwrap();
        // store the block in storage
        storage.store_block(block.clone());

        // set our block as a final block so the message is sent
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        block_storage.insert(block.id, storage.clone());
        controller.update_blockclique_status(
            finalized_blocks,
            Default::default(),
            block_storage.clone(),
        );
        // sleep for 150ms to reach the message execution period
        std::thread::sleep(Duration::from_millis(150));

        // retrieve events emitted by smart contracts
        let events = controller.get_filtered_sc_output_event(EventFilter {
            start: Some(Slot::new(1, 1)),
            end: Some(Slot::new(20, 1)),
            ..Default::default()
        });

        println!("events: {:?}", events);

        // match the events
        assert!(events.len() == 1, "One event was expected");
        assert_eq!(events[0].data, "message correctly received: 42,42,42,42");
        // stop the execution controller
        manager.stop();
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
    #[serial]
    fn test_operation_execution_status() {
        let vesting = get_initials_vesting(false);
        // setup the period duration and the maximum gas for asynchronous messages execution
        let exec_cfg = ExecutionConfig {
            max_async_gas: 100_000,
            t0: MassaTime::from_millis(100),
            cursor_delay: MassaTime::from_millis(0),
            initial_vesting_path: vesting.path().to_path_buf(),
            ..ExecutionConfig::default()
        };
        // get a sample final state
        let (sample_state, _keep_file, _keep_dir) = get_sample_state(0).unwrap();

        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

        // init the storage
        let mut storage = Storage::create_root();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        // start the execution worker
        let (mut manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());
        // keypair associated to thread 0
        let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
        // load bytecodes
        // you can check the source code of the following wasm file in massa-unit-tests-src
        let bytecode = include_bytes!("./wasm/send_message.wasm");
        let datastore_bytecode = include_bytes!("./wasm/receive_message.wasm").to_vec();
        let mut datastore = BTreeMap::new();
        datastore.insert(b"smart-contract".to_vec(), datastore_bytecode);

        // create the block contaning the smart contract execution operation
        let operation = create_execute_sc_operation(&keypair, bytecode, datastore).unwrap();
        let tested_op_id = operation.id.clone();
        storage.store_operations(vec![operation.clone()]);
        let block = create_block(
            KeyPair::generate(0).unwrap(),
            vec![operation],
            vec![],
            Slot::new(1, 0),
        )
        .unwrap();
        // store the block in storage
        storage.store_block(block.clone());

        // set our block as a final block so the message is sent
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        block_storage.insert(block.id, storage.clone());
        controller.update_blockclique_status(
            finalized_blocks,
            Default::default(),
            block_storage.clone(),
        );
        // sleep for 150ms to reach the message execution period
        std::thread::sleep(Duration::from_millis(150));

        let ops = controller.get_op_exec_status();
        dbg!(&ops);

        // match the events
        assert!(
            ops.1.contains_key(&tested_op_id),
            "Expected operation not found"
        );
        let status = ops.1.get(&tested_op_id).unwrap(); // we can unwrap, thanks to assert above
        assert!(
            status == &true,
            "Operation execution status expected to be Some(true)"
        );

        println!(
            "Operation {:?} execution status: {:?}",
            &tested_op_id, &status
        );

        // stop the execution controller
        manager.stop();
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
    #[serial]
    fn local_execution() {
        let vesting = get_initials_vesting(false);
        // setup the period duration and cursor delay
        let exec_cfg = ExecutionConfig {
            t0: MassaTime::from_millis(100),
            cursor_delay: MassaTime::from_millis(0),
            initial_vesting_path: vesting.path().to_path_buf(),
            ..ExecutionConfig::default()
        };
        // get a sample final state
        let (sample_state, _keep_file, _keep_dir) = get_sample_state(0).unwrap();

        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

        // init the storage
        let mut storage = Storage::create_root();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        // start the execution worker
        let (mut manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());
        // keypair associated to thread 0
        let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
        // load bytecodes
        // you can check the source code of the following wasm files in massa-unit-tests-src
        let exec_bytecode = include_bytes!("./wasm/local_execution.wasm");
        let call_bytecode = include_bytes!("./wasm/local_call.wasm");
        let datastore_bytecode = include_bytes!("./wasm/local_function.wasm").to_vec();
        let mut datastore = BTreeMap::new();
        datastore.insert(b"smart-contract".to_vec(), datastore_bytecode);

        // create the block contaning the operations
        let local_exec_op =
            create_execute_sc_operation(&keypair, exec_bytecode, datastore.clone()).unwrap();
        let local_call_op =
            create_execute_sc_operation(&keypair, call_bytecode, datastore).unwrap();
        storage.store_operations(vec![local_exec_op.clone(), local_call_op.clone()]);
        let block = create_block(
            KeyPair::generate(0).unwrap(),
            vec![local_exec_op.clone(), local_call_op.clone()],
            vec![],
            Slot::new(1, 0),
        )
        .unwrap();
        // store the block in storage
        storage.store_block(block.clone());

        // set our block as a final block so the message is sent
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        block_storage.insert(block.id, storage.clone());
        controller.update_blockclique_status(
            finalized_blocks,
            Default::default(),
            block_storage.clone(),
        );
        // sleep for 100ms to wait for execution
        std::thread::sleep(Duration::from_millis(100));

        // retrieve events emitted by smart contracts
        let events = controller.get_filtered_sc_output_event(EventFilter {
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
        assert!(
            // start (299_000) - fee (1000) - storage cost
            Amount::from_str("299_979").unwrap() < amount
                && amount < Amount::from_str("299_980").unwrap()
        );
        assert_eq!(events[5].context.call_stack.len(), 1);
        assert_eq!(
            events[1].context.call_stack.back().unwrap(),
            &Address::from_public_key(&keypair.get_public_key())
        );
        assert_eq!(events[6].data, "one local call completed");

        // stop the execution controller
        manager.stop();
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
    #[serial]
    fn sc_deployment() {
        let vesting = get_initials_vesting(false);
        // setup the period duration and cursor delay
        let exec_cfg = ExecutionConfig {
            t0: MassaTime::from_millis(100),
            cursor_delay: MassaTime::from_millis(0),
            initial_vesting_path: vesting.path().to_path_buf(),
            ..ExecutionConfig::default()
        };
        // get a sample final state
        let (sample_state, _keep_file, _keep_dir) = get_sample_state(0).unwrap();

        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

        // init the storage
        let mut storage = Storage::create_root();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        // start the execution worker
        let (mut manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());
        // keypair associated to thread 0
        let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
        // load bytecodes
        // you can check the source code of the following wasm files in massa-unit-tests-src
        let op_bytecode = include_bytes!("./wasm/deploy_sc.wasm");
        let datastore_bytecode = include_bytes!("./wasm/init_sc.wasm").to_vec();
        let mut datastore = BTreeMap::new();
        datastore.insert(b"smart-contract".to_vec(), datastore_bytecode);

        // create the block contaning the operation
        let op = create_execute_sc_operation(&keypair, op_bytecode, datastore.clone()).unwrap();
        storage.store_operations(vec![op.clone()]);
        let block = create_block(
            KeyPair::generate(0).unwrap(),
            vec![op],
            vec![],
            Slot::new(1, 0),
        )
        .unwrap();
        // store the block in storage
        storage.store_block(block.clone());

        // set our block as a final block so the message is sent
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        block_storage.insert(block.id, storage.clone());
        controller.update_blockclique_status(
            finalized_blocks,
            Default::default(),
            block_storage.clone(),
        );
        // sleep for 100ms to wait for execution
        std::thread::sleep(Duration::from_millis(100));

        // retrieve events emitted by smart contracts
        let events = controller.get_filtered_sc_output_event(EventFilter {
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

        // stop the execution controller
        manager.stop();
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
    #[serial]
    fn send_and_receive_async_message_with_trigger() {
        let vesting = get_initials_vesting(false);
        // setup the period duration and the maximum gas for asynchronous messages execution
        let exec_cfg = ExecutionConfig {
            t0: MassaTime::from_millis(100),
            cursor_delay: MassaTime::from_millis(0),
            max_async_gas: 1_000_000_000,
            initial_vesting_path: vesting.path().to_path_buf(),
            ..ExecutionConfig::default()
        };
        // get a sample final state
        let (sample_state, _keep_file, _keep_dir) = get_sample_state(0).unwrap();

        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

        let mut blockclique_blocks: HashMap<Slot, BlockId> = HashMap::new();
        // init the storage
        let mut storage = Storage::create_root();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        // start the execution worker
        let (mut manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());
        // keypair associated to thread 0
        let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
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
        let operation = create_execute_sc_operation(&keypair, bytecode, datastore).unwrap();
        storage.store_operations(vec![operation.clone()]);
        let block = create_block(keypair, vec![operation], vec![], Slot::new(1, 0)).unwrap();
        // store the block in storage
        storage.store_block(block.clone());

        // set our block as a final block so the message is sent
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        block_storage.insert(block.id, storage.clone());
        blockclique_blocks.insert(block.content.header.content.slot, block.id);
        controller.update_blockclique_status(
            finalized_blocks.clone(),
            Some(blockclique_blocks.clone()),
            block_storage.clone(),
        );
        // sleep for 10ms to reach the message execution period
        std::thread::sleep(Duration::from_millis(10));

        // retrieve events emitted by smart contracts
        let events = controller.get_filtered_sc_output_event(EventFilter {
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
        let operation = create_execute_sc_operation(&keypair, bytecode, datastore).unwrap();
        storage.store_operations(vec![operation.clone()]);
        let block = create_block(keypair, vec![operation], vec![], Slot::new(1, 1)).unwrap();
        // store the block in storage
        storage.store_block(block.clone());

        // set our block as a final block so the message is sent
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        block_storage.insert(block.id, storage.clone());
        blockclique_blocks.insert(block.content.header.content.slot, block.id);
        controller.update_blockclique_status(finalized_blocks.clone(), None, block_storage.clone());
        // sleep for 10ms to reach the message execution period
        std::thread::sleep(Duration::from_millis(10));

        // retrieve events emitted by smart contracts
        let events = controller.get_filtered_sc_output_event(EventFilter {
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

        let operation = create_execute_sc_operation(&keypair, bytecode, datastore).unwrap();
        storage.store_operations(vec![operation.clone()]);
        let block = create_block(keypair, vec![operation], vec![], Slot::new(1, 2)).unwrap();
        // store the block in storage
        storage.store_block(block.clone());

        // set our block as a final block so the message is sent
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        block_storage.insert(block.id, storage.clone());
        blockclique_blocks.insert(block.content.header.content.slot, block.id);
        controller.update_blockclique_status(finalized_blocks.clone(), None, block_storage.clone());
        // sleep for 1000ms to reach the message execution period
        std::thread::sleep(Duration::from_millis(1000));

        // retrieve events emitted by smart contracts
        let events = controller.get_filtered_sc_output_event(EventFilter {
            ..Default::default()
        });

        // match the events
        assert!(events.len() == 4, "4 events were expected");

        manager.stop();
    }

    #[test]
    #[serial]
    pub fn send_and_receive_transaction() {
        let vesting = get_initials_vesting(false);
        // setup the period duration
        let exec_cfg = ExecutionConfig {
            t0: MassaTime::from_millis(100),
            cursor_delay: MassaTime::from_millis(0),
            initial_vesting_path: vesting.path().to_path_buf(),
            ..ExecutionConfig::default()
        };
        // get a sample final state
        let (sample_state, _keep_file, _keep_dir) = get_sample_state(0).unwrap();

        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

        // init the storage
        let mut storage = Storage::create_root();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        // start the execution worker
        let (mut manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());
        // generate the sender_keypair and recipient_address

        let sender_keypair = KeyPair::from_str(TEST_SK_1).unwrap();
        let (recipient_address, _keypair) = get_random_address_full();

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
            &sender_keypair,
        )
        .unwrap();
        // create the block containing the transaction operation
        storage.store_operations(vec![operation.clone()]);
        let block = create_block(
            KeyPair::generate(0).unwrap(),
            vec![operation],
            vec![],
            Slot::new(1, 0),
        )
        .unwrap();
        // store the block in storage
        storage.store_block(block.clone());
        // set our block as a final block so the transaction is processed
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        block_storage.insert(block.id, storage.clone());
        controller.update_blockclique_status(
            finalized_blocks,
            Default::default(),
            block_storage.clone(),
        );
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
    fn vesting_transfer_coins() {
        let vesting = get_initials_vesting(true);
        // setup the period duration
        let exec_cfg = ExecutionConfig {
            t0: MassaTime::from_millis(100),
            cursor_delay: MassaTime::from_millis(0),
            initial_vesting_path: vesting.path().to_path_buf(),
            ..ExecutionConfig::default()
        };
        // get a sample final state
        let (sample_state, _keep_file, _keep_dir) = get_sample_state(0).unwrap();

        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

        // init the storage
        let mut storage = Storage::create_root();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        // start the execution worker
        let (mut manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());
        // generate the sender_keypair and recipient_address
        let sender_keypair = KeyPair::from_str(TEST_SK_1).unwrap();
        let sender_addr = Address::from_public_key(&sender_keypair.get_public_key());
        let (recipient_address, _keypair) = get_random_address_full();
        // create the operation
        let operation = Operation::new_verifiable(
            Operation {
                fee: Amount::zero(),
                expire_period: 10,
                op: OperationType::Transaction {
                    recipient_address,
                    amount: Amount::from_str("250000").unwrap(),
                },
            },
            OperationSerializer::new(),
            &sender_keypair,
        )
        .unwrap();
        // create the block containing the transaction operation
        storage.store_operations(vec![operation.clone()]);
        let block = create_block(
            KeyPair::generate(0).unwrap(),
            vec![operation],
            vec![],
            Slot::new(1, 0),
        )
        .unwrap();
        // store the block in storage
        storage.store_block(block.clone());
        // set our block as a final block so the transaction is processed
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        block_storage.insert(block.id, storage.clone());
        controller.update_blockclique_status(
            finalized_blocks,
            Default::default(),
            block_storage.clone(),
        );
        std::thread::sleep(Duration::from_millis(100));

        // retrieve the event emitted by the execution error
        let events = controller.get_filtered_sc_output_event(EventFilter {
            is_error: Some(true),
            ..Default::default()
        });
        dbg!(&events);
        assert!(events[0].data.contains("massa_execution_error"));
        assert!(events[0]
            .data
            .contains("We reach the vesting constraint: vesting_min_balance=100000 with value min_balance=60000"));

        // check recipient balance
        assert!(sample_state
            .read()
            .ledger
            .get_balance(&recipient_address)
            .is_none());
        // Check sender balance
        assert_eq!(
            sample_state
                .read()
                .ledger
                .get_balance(&sender_addr)
                .unwrap(),
            Amount::from_str("300000").unwrap()
        );

        // stop the execution controller
        manager.stop();
    }

    #[test]
    #[serial]
    fn vesting_max_rolls() {
        // setup the period duration
        let vesting = get_initials_vesting(true);
        let exec_cfg = ExecutionConfig {
            t0: MassaTime::from_millis(100),
            cursor_delay: MassaTime::from_millis(0),
            initial_vesting_path: vesting.path().to_path_buf(),
            ..ExecutionConfig::default()
        };

        // get a sample final state
        let (sample_state, _keep_file, _keep_dir) = get_sample_state(0).unwrap();

        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

        // init the storage
        let mut storage = Storage::create_root();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        // start the execution worker
        let (mut manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());
        // generate the keypair and its corresponding address
        let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
        let address = Address::from_public_key(&keypair.get_public_key());
        // create the operation
        // try to buy 60 rolls so (100+60) and the max rolls specified for this address in vesting is 150
        let operation = Operation::new_verifiable(
            Operation {
                fee: Amount::zero(),
                expire_period: 10,
                op: OperationType::RollBuy { roll_count: 60 },
            },
            OperationSerializer::new(),
            &keypair,
        )
        .unwrap();
        // create the block containing the roll buy operation
        storage.store_operations(vec![operation.clone()]);
        let block = create_block(
            KeyPair::generate(0).unwrap(),
            vec![operation],
            vec![],
            Slot::new(1, 0),
        )
        .unwrap();
        // store the block in storage
        storage.store_block(block.clone());
        // set our block as a final block so the purchase is processed
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        block_storage.insert(block.id, storage.clone());
        controller.update_blockclique_status(
            finalized_blocks,
            Default::default(),
            block_storage.clone(),
        );
        std::thread::sleep(Duration::from_millis(100));

        // retrieve the event emitted by the execution error
        let events = controller.get_filtered_sc_output_event(EventFilter::default());
        assert!(events[0].data.contains("massa_execution_error"));
        assert!(events[0].data.contains(
            "We reach the vesting constraint: trying to get to a total of 160 rolls but only 50 are allowed at that time by the vesting scheme"
        ));

        // check roll count of the buyer address and its balance, same as start because operation was rejected
        let sample_read = sample_state.read();
        assert_eq!(sample_read.pos_state.get_rolls_for(&address), 100);
        assert_eq!(
            sample_read.ledger.get_balance(&address).unwrap(),
            Amount::from_str("300_000").unwrap()
        );
        // stop the execution controller
        manager.stop();
    }

    #[test]
    #[serial]
    pub fn roll_buy() {
        let vesting = get_initials_vesting(false);
        // setup the period duration
        let exec_cfg = ExecutionConfig {
            t0: MassaTime::from_millis(100),
            cursor_delay: MassaTime::from_millis(0),
            initial_vesting_path: vesting.path().to_path_buf(),
            ..ExecutionConfig::default()
        };
        // get a sample final state
        let (sample_state, _keep_file, _keep_dir) = get_sample_state(0).unwrap();

        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

        // init the storage
        let mut storage = Storage::create_root();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        // start the execution worker
        let (mut manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());
        // generate the keypair and its corresponding address
        let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
        let address = Address::from_public_key(&keypair.get_public_key());
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
        storage.store_operations(vec![operation.clone()]);
        let block = create_block(
            KeyPair::generate(0).unwrap(),
            vec![operation],
            vec![],
            Slot::new(1, 0),
        )
        .unwrap();
        // store the block in storage
        storage.store_block(block.clone());
        // set our block as a final block so the purchase is processed
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        block_storage.insert(block.id, storage.clone());
        controller.update_blockclique_status(
            finalized_blocks,
            Default::default(),
            block_storage.clone(),
        );
        std::thread::sleep(Duration::from_millis(100));
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
        let vesting = get_initials_vesting(false);
        // Try to sell 10 rolls (operation 1) then 1 rolls (operation 2)
        // Check for resulting roll count + resulting deferred credits

        // setup the period duration
        let mut exec_cfg = ExecutionConfig {
            t0: MassaTime::from_millis(100),
            cursor_delay: MassaTime::from_millis(0),
            periods_per_cycle: 2,
            thread_count: 2,
            initial_vesting_path: vesting.path().to_path_buf(),
            last_start_period: 2,
            ..Default::default()
        };
        // turn off roll selling on missed block opportunities
        // otherwise balance will be credited with those sold roll (and we need to check the balance for
        // if the deferred credits are reimbursed
        exec_cfg.max_miss_ratio = Ratio::new(1, 1);

        // get a sample final state
        let (sample_state, _keep_file, _keep_dir) = get_sample_state(2).unwrap();

        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

        // init the storage
        let mut storage = Storage::create_root();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        // start the execution worker
        let (mut manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());
        // generate the keypair and its corresponding address
        let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
        let address = Address::from_public_key(&keypair.get_public_key());

        // get initial balance
        let balance_initial = sample_state.read().ledger.get_balance(&address).unwrap();

        // get initial roll count
        let roll_count_initial = sample_state.read().pos_state.get_rolls_for(&address);
        let roll_sell_1 = 10;
        let roll_sell_2 = 1;

        let initial_deferred_credits = Amount::from_str("100").unwrap();
        // set initial_deferred_credits that will be reimbursed at first block
        sample_state.write().pos_state.deferred_credits.insert(
            Slot::new(1, 0),
            address,
            initial_deferred_credits,
        );

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
        // create the block containing the roll buy operation
        storage.store_operations(vec![operation1.clone(), operation2.clone()]);
        let block = create_block(
            KeyPair::generate(0).unwrap(),
            vec![operation1, operation2],
            vec![],
            Slot::new(3, 0),
        )
        .unwrap();
        // store the block in storage
        storage.store_block(block.clone());
        // set the block as final so the sell and credits are processed
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        block_storage.insert(block.id, storage.clone());
        controller.update_blockclique_status(
            finalized_blocks,
            Default::default(),
            block_storage.clone(),
        );
        std::thread::sleep(Duration::from_millis(1000));

        // check roll count deferred credits and candidate balance of the seller address
        let sample_read = sample_state.read();
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
        let balances = controller.get_final_and_candidate_balance(&[address]);
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

        // stop the execution controller
        manager.stop();
    }

    #[test]
    #[serial]
    pub fn roll_slash() {
        let vesting = get_initials_vesting(false);
        // Try to sell 97 rolls (operation 1) then process a Denunciation (with config set to slash
        // 3 rolls)
        // Check for resulting roll & deferred credits & balance

        // setup the period duration
        let mut exec_cfg = ExecutionConfig {
            t0: MassaTime::from_millis(100),
            cursor_delay: MassaTime::from_millis(0),
            periods_per_cycle: 2,
            thread_count: 2,
            initial_vesting_path: vesting.path().to_path_buf(),
            last_start_period: 2,
            roll_count_to_slash_on_denunciation: 3, // Set to 3 to check if config is taken into account
            ..Default::default()
        };
        // turn off roll selling on missed block opportunities
        // otherwise balance will be credited with those sold roll (and we need to check the balance for
        // if the deferred credits are reimbursed
        exec_cfg.max_miss_ratio = Ratio::new(1, 1);

        // get a sample final state
        let (sample_state, _keep_file, _keep_dir) =
            get_sample_state(exec_cfg.last_start_period).unwrap();

        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

        // init the storage
        let mut storage = Storage::create_root();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        // start the execution worker
        let (mut manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());

        // generate the keypair and its corresponding address
        let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
        let address = Address::from_public_key(&keypair.get_public_key());

        // get initial balance
        let balance_initial = sample_state.read().ledger.get_balance(&address).unwrap();

        // get initial roll count
        let roll_count_initial = sample_state.read().pos_state.get_rolls_for(&address);
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
        let (_slot, _keypair, s_endorsement_1, s_endorsement_2, _) =
            gen_endorsements_for_denunciation(
                Some(Slot::new(exec_cfg.last_start_period, 4)),
                Some(keypair),
            );
        let denunciation_2 = Denunciation::try_from((&s_endorsement_1, &s_endorsement_2)).unwrap();

        // create the block containing the roll buy operation
        storage.store_operations(vec![operation1.clone()]);
        let block = create_block(
            KeyPair::generate(0).unwrap(),
            vec![operation1],
            vec![denunciation.clone(), denunciation, denunciation_2],
            Slot::new(3, 0),
        )
        .unwrap();
        // store the block in storage
        storage.store_block(block.clone());
        // set the block as final so the sell and credits are processed
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        block_storage.insert(block.id, storage.clone());
        controller.update_blockclique_status(
            finalized_blocks,
            Default::default(),
            block_storage.clone(),
        );
        std::thread::sleep(Duration::from_millis(1000));

        // check roll count deferred credits and candidate balance of the seller address
        let sample_read = sample_state.read();
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
        let balances = controller.get_final_and_candidate_balance(&[address]);
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

        // stop the execution controller
        manager.stop();
    }

    #[test]
    #[serial]
    pub fn roll_slash_2() {
        let vesting = get_initials_vesting(false);
        // Try to sell all rolls (operation 1) then process a Denunciation (with config set to slash
        // 4 rolls)
        // Check for resulting roll & deferred credits & balance

        // setup the period duration
        let mut exec_cfg = ExecutionConfig {
            t0: MassaTime::from_millis(100),
            cursor_delay: MassaTime::from_millis(0),
            periods_per_cycle: 2,
            thread_count: 2,
            initial_vesting_path: vesting.path().to_path_buf(),
            last_start_period: 2,
            roll_count_to_slash_on_denunciation: 4, // Set to 4 to check if config is taken into account
            ..Default::default()
        };
        // turn off roll selling on missed block opportunities
        // otherwise balance will be credited with those sold roll (and we need to check the balance for
        // if the deferred credits are reimbursed
        exec_cfg.max_miss_ratio = Ratio::new(1, 1);

        // get a sample final state
        let (sample_state, _keep_file, _keep_dir) =
            get_sample_state(exec_cfg.last_start_period).unwrap();

        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

        // init the storage
        let mut storage = Storage::create_root();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        // start the execution worker
        let (mut manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());

        // generate the keypair and its corresponding address
        let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
        let address = Address::from_public_key(&keypair.get_public_key());

        // get initial balance
        let balance_initial = sample_state.read().ledger.get_balance(&address).unwrap();

        // get initial roll count
        let roll_count_initial = sample_state.read().pos_state.get_rolls_for(&address);
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
            gen_endorsements_for_denunciation(Some(Slot::new(3, 0)), Some(keypair));
        let denunciation = Denunciation::try_from((&s_endorsement_1, &s_endorsement_2)).unwrap();

        // create the block containing the roll buy operation
        storage.store_operations(vec![operation1.clone(), operation2.clone()]);
        let block = create_block(
            KeyPair::generate(0).unwrap(),
            vec![operation1, operation2],
            vec![denunciation.clone(), denunciation],
            Slot::new(3, 0),
        )
        .unwrap();
        // store the block in storage
        storage.store_block(block.clone());
        // set the block as final so the sell and credits are processed
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        block_storage.insert(block.id, storage.clone());
        controller.update_blockclique_status(
            finalized_blocks,
            Default::default(),
            block_storage.clone(),
        );
        std::thread::sleep(Duration::from_millis(1000));

        // check roll count & deferred credits & candidate balance
        let sample_read = sample_state.read();
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
        let balances = controller.get_final_and_candidate_balance(&[address]);
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

        // stop the execution controller
        manager.stop();
    }

    #[test]
    #[serial]
    fn sc_execution_error() {
        let vesting = get_initials_vesting(false);
        // setup the period duration and the maximum gas for asynchronous messages execution
        let exec_cfg = ExecutionConfig {
            t0: MassaTime::from_millis(100),
            cursor_delay: MassaTime::from_millis(0),
            max_async_gas: 100_000,
            initial_vesting_path: vesting.path().to_path_buf(),
            ..ExecutionConfig::default()
        };
        // get a sample final state
        let (sample_state, _keep_file, _keep_dir) = get_sample_state(0).unwrap();

        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

        // init the storage
        let mut storage = Storage::create_root();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        // start the execution worker
        let (mut manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());
        // keypair associated to thread 0
        let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
        // load bytecode
        // you can check the source code of the following wasm file in massa-unit-tests-src
        let bytecode = include_bytes!("./wasm/execution_error.wasm");
        // create the block containing the erroneous smart contract execution operation
        let operation =
            create_execute_sc_operation(&keypair, bytecode, BTreeMap::default()).unwrap();
        storage.store_operations(vec![operation.clone()]);
        let block = create_block(
            KeyPair::generate(0).unwrap(),
            vec![operation],
            vec![],
            Slot::new(1, 0),
        )
        .unwrap();
        // store the block in storage
        storage.store_block(block.clone());
        // set our block as a final block
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        block_storage.insert(block.id, storage.clone());
        controller.update_blockclique_status(
            finalized_blocks,
            Default::default(),
            block_storage.clone(),
        );
        std::thread::sleep(Duration::from_millis(100));

        // retrieve the event emitted by the execution error
        let events = controller.get_filtered_sc_output_event(EventFilter {
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
        // stop the execution controller
        manager.stop();
    }

    #[test]
    #[serial]
    fn sc_datastore() {
        let vesting = get_initials_vesting(false);
        // setup the period duration and the maximum gas for asynchronous messages execution
        let exec_cfg = ExecutionConfig {
            t0: MassaTime::from_millis(100),
            cursor_delay: MassaTime::from_millis(0),
            max_async_gas: 100_000,
            initial_vesting_path: vesting.path().to_path_buf(),
            ..ExecutionConfig::default()
        };
        // get a sample final state
        let (sample_state, _keep_file, _keep_dir) = get_sample_state(0).unwrap();

        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

        // init the storage
        let mut storage = Storage::create_root();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        // start the execution worker
        let (mut manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());
        // keypair associated to thread 0
        let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
        // load bytecode
        // you can check the source code of the following wasm file in massa-unit-tests-src
        let bytecode = include_bytes!("./wasm/datastore.wasm");
        let datastore = BTreeMap::from([(vec![65, 66], vec![255]), (vec![9], vec![10, 11])]);

        // create the block containing the erroneous smart contract execution operation
        let operation = create_execute_sc_operation(&keypair, bytecode, datastore).unwrap();
        storage.store_operations(vec![operation.clone()]);
        let block = create_block(
            KeyPair::generate(0).unwrap(),
            vec![operation],
            vec![],
            Slot::new(1, 0),
        )
        .unwrap();
        // store the block in storage
        storage.store_block(block.clone());
        // set our block as a final block
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        block_storage.insert(block.id, storage.clone());
        controller.update_blockclique_status(
            finalized_blocks,
            Some(Default::default()),
            block_storage,
        );
        std::thread::sleep(Duration::from_millis(100));

        // retrieve the event emitted by the execution error
        let events = controller.get_filtered_sc_output_event(EventFilter::default());

        // match the events
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].data, "keys: 9,65,66");
        assert_eq!(events[1].data, "has_key_1: true - has_key_2: false");
        assert_eq!(events[2].data, "data key 1: 255 - data key 3: 10,11");

        // stop the execution controller
        manager.stop();
    }

    #[test]
    #[serial]
    fn set_bytecode_error() {
        let vesting = get_initials_vesting(false);
        // setup the period duration and the maximum gas for asynchronous messages execution
        let exec_cfg = ExecutionConfig {
            t0: MassaTime::from_millis(100),
            cursor_delay: MassaTime::from_millis(0),
            max_async_gas: 100_000,
            initial_vesting_path: vesting.path().to_path_buf(),
            ..ExecutionConfig::default()
        };
        // get a sample final state
        let (sample_state, _keep_file, _keep_dir) = get_sample_state(0).unwrap();

        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

        // init the storage
        let mut storage = Storage::create_root();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        // start the execution worker
        let (_manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());
        // keypair associated to thread 0
        let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
        // load bytecodes
        // you can check the source code of the following wasm file in massa-unit-tests-src
        let bytecode = include_bytes!("./wasm/set_bytecode_fail.wasm");
        let datastore_bytecode = include_bytes!("./wasm/smart-contract.wasm").to_vec();
        let mut datastore = BTreeMap::new();
        datastore.insert(b"smart-contract".to_vec(), datastore_bytecode);

        // create the block containing the erroneous smart contract execution operation
        let operation = create_execute_sc_operation(&keypair, bytecode, datastore).unwrap();
        storage.store_operations(vec![operation.clone()]);
        let block = create_block(
            KeyPair::generate(0).unwrap(),
            vec![operation],
            vec![],
            Slot::new(1, 0),
        )
        .unwrap();
        // store the block in storage
        storage.store_block(block.clone());
        // set our block as a final block
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        block_storage.insert(block.id, storage.clone());
        controller.update_blockclique_status(
            finalized_blocks,
            Default::default(),
            block_storage.clone(),
        );
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
        let vesting = get_initials_vesting(false);
        // setup the period duration and the maximum gas for asynchronous messages execution
        let exec_cfg = ExecutionConfig {
            t0: MassaTime::from_millis(100),
            cursor_delay: MassaTime::from_millis(0),
            max_async_gas: 100_000,
            initial_vesting_path: vesting.path().to_path_buf(),
            ..ExecutionConfig::default()
        };
        // get a sample final state
        let (sample_state, _keep_file, _keep_dir) = get_sample_state(0).unwrap();

        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

        // init the storage
        let mut storage = Storage::create_root();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        // start the execution worker
        let (mut manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());

        // keypair associated to thread 0
        let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
        // let address = Address::from_public_key(&keypair.get_public_key());

        // load bytecode
        // you can check the source code of the following wasm file in massa-unit-tests-src
        let bytecode = include_bytes!("./wasm/datastore_manipulations.wasm");
        // create the block containing the erroneous smart contract execution operation
        let operation =
            create_execute_sc_operation(&keypair, bytecode, BTreeMap::default()).unwrap();
        storage.store_operations(vec![operation.clone()]);
        let block = create_block(
            KeyPair::generate(0).unwrap(),
            vec![operation],
            vec![],
            Slot::new(1, 0),
        )
        .unwrap();
        // store the block in storage
        storage.store_block(block.clone());
        // set our block as a final block
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let block_store = vec![(block.id, storage.clone())].into_iter().collect();
        controller.update_blockclique_status(finalized_blocks, Default::default(), block_store);
        std::thread::sleep(
            exec_cfg
                .t0
                .saturating_add(MassaTime::from_millis(50))
                .into(),
        );

        let events = controller.get_filtered_sc_output_event(EventFilter::default());
        // match the events
        println!("{:?}", events);
        assert_eq!(events.len(), 4, "Got {} events, expected 4", events.len());
        let key_a: Vec<u8> = [1, 0, 4, 255].iter().cloned().collect();
        let key_a_str: String = key_a
            .iter()
            .map(|b| format!("{}", b))
            .collect::<Vec<String>>()
            .join(",");

        let key_b: Vec<u8> = [2, 0, 254, 255].iter().cloned().collect();
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

        // Length of the value left in the datastore. See sources for more context.
        let value_len = ([21, 0, 49].len() + [5, 12, 241].len()) as u64;

        assert_eq!(
            sample_state
                .read()
                .ledger
                .get_balance(&Address::from_public_key(&keypair.get_public_key()))
                .unwrap(),
            Amount::from_str("300000")
                .unwrap()
                // Gas fee
                .saturating_sub(Amount::from_mantissa_scale(10, 0))
                // Storage cost key
                .saturating_sub(
                    exec_cfg
                        .storage_costs_constants
                        .ledger_cost_per_byte
                        .saturating_mul_u64(2 * LEDGER_ENTRY_DATASTORE_BASE_SIZE as u64)
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

    /// This test checks causes a history rewrite in slot sequencing and ensures that emitted events match
    #[test]
    #[serial]
    fn events_from_switching_blockclique() {
        let vesting = get_initials_vesting(false);
        // Compile the `./wasm_tests` and generate a block with `event_test.wasm`
        // as data. Then we check if we get an event as expected.
        let exec_cfg = ExecutionConfig {
            t0: MassaTime::from_millis(100),
            cursor_delay: MassaTime::from_millis(0),
            initial_vesting_path: vesting.path().to_path_buf(),
            ..ExecutionConfig::default()
        };
        let storage: Storage = Storage::create_root();
        let (sample_state, _keep_file, _keep_dir) = get_sample_state(0).unwrap();
        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        let (mut manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());

        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        let mut blockclique_blocks: HashMap<Slot, BlockId> = HashMap::new();

        // create blockclique block at slot (1,1)
        {
            let blockclique_block_slot = Slot::new(1, 1);
            let keypair = KeyPair::from_str(TEST_SK_2).unwrap();
            let event_test_data = include_bytes!("./wasm/event_test.wasm");
            let operation =
                create_execute_sc_operation(&keypair, event_test_data, BTreeMap::default())
                    .unwrap();
            let blockclique_block = create_block(
                keypair,
                vec![operation.clone()],
                vec![],
                blockclique_block_slot,
            )
            .unwrap();
            blockclique_blocks.insert(blockclique_block_slot, blockclique_block.id);
            let mut blockclique_block_storage = storage.clone_without_refs();
            blockclique_block_storage.store_block(blockclique_block.clone());
            blockclique_block_storage.store_operations(vec![operation]);
            block_storage.insert(blockclique_block.id, blockclique_block_storage);
        }
        // notify execution about blockclique change
        controller.update_blockclique_status(
            Default::default(),
            Some(blockclique_blocks.clone()),
            block_storage.clone(),
        );
        std::thread::sleep(Duration::from_millis(1000));
        let events = controller.get_filtered_sc_output_event(EventFilter::default());
        assert_eq!(events.len(), 1, "wrong event count");
        assert_eq!(events[0].context.slot, Slot::new(1, 1), "Wrong event slot");

        // create blockclique block at slot (1,0)
        {
            let blockclique_block_slot = Slot::new(1, 0);
            let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
            let event_test_data = include_bytes!("./wasm/event_test.wasm");
            let operation =
                create_execute_sc_operation(&keypair, event_test_data, BTreeMap::default())
                    .unwrap();
            let blockclique_block = create_block(
                keypair,
                vec![operation.clone()],
                vec![],
                blockclique_block_slot,
            )
            .unwrap();
            blockclique_blocks.insert(blockclique_block_slot, blockclique_block.id);
            let mut blockclique_block_storage = storage.clone_without_refs();
            blockclique_block_storage.store_block(blockclique_block.clone());
            blockclique_block_storage.store_operations(vec![operation]);
            block_storage.insert(blockclique_block.id, blockclique_block_storage);
        }
        // notify execution about blockclique change
        controller.update_blockclique_status(
            Default::default(),
            Some(blockclique_blocks.clone()),
            block_storage.clone(),
        );
        std::thread::sleep(Duration::from_millis(1000));
        let events = controller.get_filtered_sc_output_event(EventFilter::default());
        assert_eq!(events.len(), 2, "wrong event count");
        assert_eq!(events[0].context.slot, Slot::new(1, 0), "Wrong event slot");
        assert_eq!(events[1].context.slot, Slot::new(1, 1), "Wrong event slot");

        manager.stop();
    }

    #[test]
    #[serial]
    pub fn not_enough_compilation_gas() {
        let vesting = get_initials_vesting(false);
        // config
        let exec_cfg = ExecutionConfig {
            t0: MassaTime::from_millis(100),
            initial_vesting_path: vesting.path().to_path_buf(),
            ..ExecutionConfig::default()
        };
        // get a sample final state
        let (sample_state, _keep_file, _keep_dir) = get_sample_state(0).unwrap();

        // init the storage
        let mut storage = Storage::create_root();
        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        // start the execution worker
        let (mut manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());

        // keypair associated to thread 0
        let keypair = KeyPair::from_str(TEST_SK_1).unwrap();

        // load bytecode
        // you can check the source code of the following wasm file in massa-unit-tests-src
        let bytecode = include_bytes!("./wasm/datastore_manipulations.wasm");
        // create the block containing the operation
        let operation = Operation::new_verifiable(
            Operation {
                fee: Amount::from_mantissa_scale(10, 0),
                expire_period: 10,
                op: OperationType::ExecuteSC {
                    max_coins: Amount::from_mantissa_scale(0, 0),
                    data: bytecode.to_vec(),
                    max_gas: 0,
                    datastore: BTreeMap::default(),
                },
            },
            OperationSerializer::new(),
            &keypair,
        )
        .unwrap();
        storage.store_operations(vec![operation.clone()]);
        let block = create_block(
            KeyPair::generate(0).unwrap(),
            vec![operation],
            vec![],
            Slot::new(1, 0),
        )
        .unwrap();
        // store the block in storage
        storage.store_block(block.clone());
        // set our block as a final block
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let block_store = vec![(block.id, storage.clone())].into_iter().collect();

        // update blockclique
        controller.update_blockclique_status(finalized_blocks, Default::default(), block_store);
        std::thread::sleep(Duration::from_millis(100));

        // assert events
        let events = controller.get_filtered_sc_output_event(EventFilter::default());
        assert!(events[0]
            .data
            .contains("not enough gas to pay for singlepass compilation"));

        manager.stop();
    }

    /// Create an operation for the given sender with `data` as bytecode.
    /// Return a result that should be unwrapped in the root `#[test]` routine.
    fn create_execute_sc_operation(
        sender_keypair: &KeyPair,
        data: &[u8],
        datastore: Datastore,
    ) -> Result<SecureShareOperation, ExecutionError> {
        let op = OperationType::ExecuteSC {
            data: data.to_vec(),
            max_gas: 100_000_000,
            max_coins: Amount::from_str("5000000").unwrap(),
            datastore,
        };
        let op = Operation::new_verifiable(
            Operation {
                fee: Amount::from_mantissa_scale(10, 0),
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
        fee: Amount,
        coins: Amount,
        target_addr: Address,
        target_func: String,
        param: Vec<u8>,
    ) -> Result<SecureShareOperation, ExecutionError> {
        let op = OperationType::CallSC {
            max_gas,
            target_addr,
            coins,
            target_func,
            param,
        };
        let op = Operation::new_verifiable(
            Operation {
                fee,
                expire_period: 10,
                op,
            },
            OperationSerializer::new(),
            sender_keypair,
        )?;
        Ok(op)
    }

    #[test]
    #[serial]
    fn sc_builtins() {
        let vesting = get_initials_vesting(false);
        // setup the period duration and the maximum gas for asynchronous messages execution
        let exec_cfg = ExecutionConfig {
            t0: MassaTime::from_millis(100),
            cursor_delay: MassaTime::from_millis(0),
            max_async_gas: 100_000,
            initial_vesting_path: vesting.path().to_path_buf(),
            ..ExecutionConfig::default()
        };
        // get a sample final state
        let (sample_state, _keep_file, _keep_dir) = get_sample_state(0).unwrap();

        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();
        // init the storage
        let mut storage = Storage::create_root();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        // start the execution worker
        let (mut manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());
        // keypair associated to thread 0
        let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
        // load bytecode
        // you can check the source code of the following wasm file in massa-unit-tests-src
        let bytecode = include_bytes!("./wasm/use_builtins.wasm");
        // create the block containing the erroneous smart contract execution operation
        let operation =
            create_execute_sc_operation(&keypair, bytecode, BTreeMap::default()).unwrap();
        storage.store_operations(vec![operation.clone()]);
        let block = create_block(
            KeyPair::generate(0).unwrap(),
            vec![operation],
            vec![],
            Slot::new(1, 0),
        )
        .unwrap();
        // store the block in storage
        storage.store_block(block.clone());
        // set our block as a final block
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_storage: PreHashMap<BlockId, Storage> = Default::default();
        block_storage.insert(block.id, storage.clone());
        controller.update_blockclique_status(
            finalized_blocks,
            Default::default(),
            block_storage.clone(),
        );
        std::thread::sleep(Duration::from_millis(10));

        // retrieve the event emitted by the execution error
        let events = controller.get_filtered_sc_output_event(EventFilter::default());
        // match the events
        assert!(!events.is_empty(), "One event was expected");
        assert!(events[0].data.contains("massa_execution_error"));
        assert!(events[0]
            .data
            .contains("runtime error when executing operation"));
        assert!(events[0]
            .data
            .contains("abord with date and rnd at use_builtins.ts:0 col: 0"));

        assert_eq!(
            sample_state
                .read()
                .ledger
                .get_balance(&Address::from_public_key(&keypair.get_public_key()))
                .unwrap(),
            Amount::from_str("299990").unwrap()
        );
        // stop the execution controller
        manager.stop();
    }

    #[test]
    #[serial]
    fn validate_address() {
        let vesting = get_initials_vesting(false);
        // setup the period duration and the maximum gas for asynchronous messages execution
        let exec_cfg = ExecutionConfig {
            t0: MassaTime::from_millis(100),
            cursor_delay: MassaTime::from_millis(0),
            max_async_gas: 100_000,
            initial_vesting_path: vesting.path().to_path_buf(),
            ..ExecutionConfig::default()
        };
        // get a sample final state
        let (sample_state, _keep_file, _keep_dir) = get_sample_state(0).unwrap();

        // init the MIP store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();
        // init the storage
        let mut storage = Storage::create_root();

        let slot_execution_output_sender = broadcast::channel(5000).0;

        let channels = ExecutionChannels {
            slot_execution_output_sender,
        };

        // start the execution worker
        let (mut manager, controller) = start_execution_worker(
            exec_cfg.clone(),
            sample_state.clone(),
            sample_state.read().pos_state.selector.clone(),
            mip_store,
            channels,
        );
        // initialize the execution system with genesis blocks
        init_execution_worker(&exec_cfg, &storage, controller.clone());

        // keypair associated to thread 0
        let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
        // let address = Address::from_public_key(&keypair.get_public_key());

        // load bytecode
        // you can check the source code of the following wasm file in massa-unit-tests-src
        let bytecode = include_bytes!("./wasm/validate_address.wasm");

        // create the block containing the erroneous smart contract execution operation
        let operation =
            create_execute_sc_operation(&keypair, bytecode, BTreeMap::default()).unwrap();
        storage.store_operations(vec![operation.clone()]);
        let block = create_block(
            KeyPair::generate(0).unwrap(),
            vec![operation],
            vec![],
            Slot::new(1, 0),
        )
        .unwrap();

        // store the block in storage
        storage.store_block(block.clone());

        // set our block as a final block
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let block_store = vec![(block.id, storage.clone())].into_iter().collect();
        controller.update_blockclique_status(finalized_blocks, Default::default(), block_store);
        std::thread::sleep(
            exec_cfg
                .t0
                .saturating_add(MassaTime::from_millis(50))
                .into(),
        );

        let events = controller.get_filtered_sc_output_event(EventFilter::default());
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
            sample_state
                .read()
                .ledger
                .get_balance(&Address::from_public_key(&keypair.get_public_key()))
                .unwrap(),
            Amount::from_str("300000")
                .unwrap()
                // Gas fee
                .saturating_sub(Amount::from_mantissa_scale(10, 0))
        );

        // stop the execution controller
        manager.stop();
    }
}
