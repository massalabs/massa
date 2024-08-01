// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_async_pool::{AsyncMessage, AsyncPool, AsyncPoolChanges, AsyncPoolConfig};
use massa_db_exports::{DBBatch, ShareableMassaDBController};
use massa_executed_ops::{ExecutedDenunciations, ExecutedDenunciationsConfig};
use massa_execution_exports::{
    ExecutionConfig, ExecutionQueryRequest, ExecutionQueryRequestItem, ExecutionStackElement,
    ReadOnlyExecutionRequest, ReadOnlyExecutionTarget,
};
use massa_final_state::test_exports::get_initials;
use massa_final_state::MockFinalStateController;
use massa_hash::Hash;
use massa_ledger_exports::{
    LedgerEntryUpdate, MockLedgerControllerWrapper, SetOrKeep, SetUpdateOrDelete,
};
use massa_models::bytecode::Bytecode;
use massa_models::config::{
    CHAINID, ENDORSEMENT_COUNT, LEDGER_ENTRY_DATASTORE_BASE_SIZE, MIP_STORE_STATS_BLOCK_CONSIDERED,
    THREAD_COUNT,
};
use massa_models::prehash::PreHashMap;
use massa_models::test_exports::gen_endorsements_for_denunciation;
use massa_models::{address::Address, amount::Amount, slot::Slot};
use massa_models::{
    denunciation::Denunciation,
    execution::EventFilter,
    operation::{Operation, OperationSerializer, OperationType},
    secure_share::SecureShareContent,
};
use massa_pos_exports::{
    CycleInfo, MockSelectorControllerWrapper, PoSConfig, PoSFinalState, ProductionStats, Selection,
};
use massa_signature::KeyPair;
use massa_test_framework::{TestUniverse, WaitPoint};
use massa_versioning::mips::get_mip_list;
use massa_versioning::versioning::{MipStatsConfig, MipStore};
use mockall::predicate;
use num::rational::Ratio;
use parking_lot::RwLock;
use std::sync::Arc;
use std::{cmp::Reverse, collections::BTreeMap, str::FromStr, time::Duration};

use super::universe::{ExecutionForeignControllers, ExecutionTestUniverse};

#[cfg(feature = "execution-trace")]
use massa_execution_exports::types_trace_info::AbiTrace;
#[cfg(feature = "execution-trace")]
use massa_execution_exports::{SCRuntimeAbiTraceType, SCRuntimeAbiTraceValue};
#[cfg(feature = "execution-trace")]
use massa_models::operation::OperationId;
#[cfg(feature = "execution-trace")]
use std::thread;

#[cfg(feature = "dump-block")]
use massa_proto_rs::massa::model::v1::FilledBlock;
#[cfg(feature = "dump-block")]
use prost::Message;
#[cfg(feature = "dump-block")]
use std::io::Cursor;

const TEST_SK_1: &str = "S18r2i8oJJyhF7Kprx98zwxAc3W4szf7RKuVMX6JydZz8zSxHeC";
const TEST_SK_2: &str = "S1FpYC4ugG9ivZZbLVrTwWtF9diSRiAwwrVX5Gx1ANSRLfouUjq";
const TEST_SK_3: &str = "S1LgXhWLEgAgCX3nm6y8PVPzpybmsYpi6yg6ZySwu5Z4ERnD7Bu";

fn final_state_boilerplate(
    mock_final_state: &mut Arc<RwLock<MockFinalStateController>>,
    db: ShareableMassaDBController,
    selector_controller: &MockSelectorControllerWrapper,
    ledger_controller: &mut MockLedgerControllerWrapper,
    saved_bytecode: Option<Arc<RwLock<Option<Bytecode>>>>,
    custom_async_pool: Option<AsyncPool>,
    custom_pos_state: Option<PoSFinalState>,
) {
    mock_final_state
        .write()
        .expect_get_slot()
        .times(1)
        .returning(move || Slot::new(0, 0));

    mock_final_state
        .write()
        .expect_get_execution_trail_hash()
        .returning(|| Hash::compute_from("Genesis".as_bytes()));

    let pos_final_state = custom_pos_state.unwrap_or_else(|| {
        let (rolls_path, _) = get_initials();
        let mut batch = DBBatch::default();
        let mut pos_final_state = PoSFinalState::new(
            PoSConfig::default(),
            "",
            &rolls_path.into_temp_path().to_path_buf(),
            Box::new(selector_controller.clone()),
            db.clone(),
        )
        .unwrap();
        pos_final_state.create_initial_cycle(&mut batch);
        db.write().write_batch(batch, Default::default(), None);
        pos_final_state
    });

    mock_final_state
        .write()
        .expect_get_pos_state()
        .return_const(pos_final_state);

    let mip_stats_config = MipStatsConfig {
        block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
        warn_announced_version_ratio: Ratio::new_raw(30, 100),
    };
    let mip_list = get_mip_list();
    let mip_store =
        MipStore::try_from((mip_list, mip_stats_config)).expect("mip store creation failed");

    mock_final_state
        .write()
        .expect_get_mip_store()
        .return_const(mip_store);

    let async_pool =
        custom_async_pool.unwrap_or(AsyncPool::new(AsyncPoolConfig::default(), db.clone()));
    mock_final_state
        .write()
        .expect_get_async_pool()
        .return_const(async_pool);

    ledger_controller.set_expectations(|ledger_controller| {
        ledger_controller
            .expect_get_balance()
            .returning(move |_| Some(Amount::from_str("100").unwrap()));
        if let Some(saved_bytecode) = saved_bytecode {
            ledger_controller
                .expect_get_bytecode()
                .returning(move |_| saved_bytecode.read().clone());
        }
        ledger_controller
            .expect_entry_exists()
            .returning(move |_| false);
    });
    mock_final_state
        .write()
        .expect_get_ledger()
        .return_const(Box::new(ledger_controller.clone()));
    mock_final_state
        .write()
        .expect_executed_ops_contains()
        .return_const(false);
    mock_final_state
        .write()
        .expect_get_executed_denunciations()
        .return_const(ExecutedDenunciations::new(
            ExecutedDenunciationsConfig {
                denunciation_expire_periods: 10,
                thread_count: THREAD_COUNT,
                endorsement_count: ENDORSEMENT_COUNT,
                keep_executed_history_extra_periods: 10,
            },
            db.clone(),
        ));
}

fn expect_finalize_deploy_and_call_blocks(
    deploy_sc_slot: Slot,
    call_sc_slot: Option<Slot>,
    finalized_waitpoint_trigger_handle: WaitPoint,
    mock_final_state: &mut Arc<RwLock<MockFinalStateController>>,
) -> Arc<RwLock<Option<Bytecode>>> {
    let saved_bytecode = Arc::new(RwLock::new(None));
    let saved_bytecode_edit = saved_bytecode.clone();
    let finalized_waitpoint_trigger_handle_2 =
        finalized_waitpoint_trigger_handle.get_trigger_handle();
    mock_final_state
        .write()
        .expect_finalize()
        .times(1)
        .with(predicate::eq(deploy_sc_slot), predicate::always())
        .returning(move |_, changes| {
            let mut saved_bytecode = saved_bytecode_edit.write();
            if !changes.ledger_changes.get_bytecode_updates().is_empty() {
                *saved_bytecode = Some(changes.ledger_changes.get_bytecode_updates()[0].clone());
            }
            finalized_waitpoint_trigger_handle.trigger();
        });
    if let Some(call_sc_slot) = call_sc_slot {
        mock_final_state
            .write()
            .expect_finalize()
            .times(1)
            .with(predicate::eq(call_sc_slot), predicate::always())
            .returning(move |_, _| {
                finalized_waitpoint_trigger_handle_2.trigger();
            });
    }
    saved_bytecode
}

fn selector_boilerplate(mock_selector: &mut MockSelectorControllerWrapper) {
    mock_selector.set_expectations(|selector_controller| {
        selector_controller
            .expect_feed_cycle()
            .returning(move |_, _, _| Ok(()));
        selector_controller
            .expect_wait_for_draws()
            .returning(move |cycle| Ok(cycle + 1));
        selector_controller
            .expect_get_producer()
            .returning(move |_| {
                Ok(Address::from_public_key(
                    &KeyPair::from_str(TEST_SK_1).unwrap().get_public_key(),
                ))
            });
    });
}

#[test]
fn test_execution_shutdown() {
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        None,
        None,
        None,
    );
    ExecutionTestUniverse::new(foreign_controllers, ExecutionConfig::default());
}

#[test]
fn test_sending_command() {
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        None,
        None,
        None,
    );
    let universe = ExecutionTestUniverse::new(foreign_controllers, ExecutionConfig::default());
    universe.module_controller.update_blockclique_status(
        Default::default(),
        Default::default(),
        Default::default(),
    );
}

#[test]
fn test_readonly_execution() {
    // setup the period duration
    let exec_cfg = ExecutionConfig::default();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(&mut foreign_controllers.selector_controller);

    foreign_controllers
        .ledger_controller
        .set_expectations(|ledger_controller| {
            ledger_controller.expect_get_bytecode().returning(move |_| {
                Some(Bytecode(
                    include_bytes!("./wasm/get_call_coins_test.wasm").to_vec(),
                ))
            });
        });

    foreign_controllers
        .ledger_controller
        .set_expectations(|ledger_controller| {
            ledger_controller
                .expect_get_balance()
                .returning(move |_| Some(Amount::from_str("100").unwrap()));
            ledger_controller
                .expect_entry_exists()
                .times(1)
                .returning(move |_| true);
        });
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        None,
        None,
        None,
    );
    let universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);

    let addr = Address::from_str("AU1LQrXPJ3DVL8SFRqACk31E9MVxBcmCATFiRdpEmgztGxWAx48D").unwrap();

    let mut res = universe
        .module_controller
        .execute_readonly_request(ReadOnlyExecutionRequest {
            max_gas: 100_000_000,
            call_stack: vec![ExecutionStackElement {
                address: addr,
                coins: Amount::zero(),
                owned_addresses: vec![],
                operation_datastore: None,
            }],
            target: ReadOnlyExecutionTarget::BytecodeExecution(
                include_bytes!("./wasm/event_test.wasm").to_vec(),
            ),
            coins: None,
            fee: Some(Amount::from_str("40").unwrap()),
        })
        .expect("readonly execution failed");

    assert!(res.gas_cost > 0);
    assert_eq!(res.out.events.take().len(), 1, "wrong number of events");
    assert_eq!(
        res.out.state_changes.ledger_changes.0.get(&addr).unwrap(),
        &SetUpdateOrDelete::Update(LedgerEntryUpdate {
            balance: massa_ledger_exports::SetOrKeep::Set(Amount::from_str("60").unwrap()),
            bytecode: massa_ledger_exports::SetOrKeep::Keep,
            datastore: BTreeMap::new()
        })
    );

    let mut res2 = universe
        .module_controller
        .execute_readonly_request(ReadOnlyExecutionRequest {
            max_gas: 414_000_000, // 314_000_000 (SP COMPIL) + 100_000_000 (FOR EXECUTION)
            call_stack: vec![
                ExecutionStackElement {
                    address: addr,
                    coins: Amount::zero(),
                    owned_addresses: vec![],
                    operation_datastore: None,
                },
                ExecutionStackElement {
                    address: Address::from_str(
                        "AU1DHJY6zd6oKJPos8gQ6KYqmsTR669wes4ZhttLD9gE7PYUF3Rs",
                    )
                    .unwrap(),
                    coins: Amount::zero(),
                    owned_addresses: vec![],
                    operation_datastore: None,
                },
            ],
            target: ReadOnlyExecutionTarget::FunctionCall {
                target_addr: Address::from_str(
                    "AS12mzL2UWroPV7zzHpwHnnF74op9Gtw7H55fAmXMnCuVZTFSjZCA",
                )
                .unwrap(),
                target_func: "test".to_string(),
                parameter: vec![],
            },
            coins: Some(Amount::from_str("20").unwrap()),
            fee: Some(Amount::from_str("30").unwrap()),
        })
        .expect("readonly execution failed");

    //assert_eq!(res2.out.slot, Slot::new(0, 1));
    assert!(res2.gas_cost > 0);
    assert_eq!(res2.out.events.take().len(), 1, "wrong number of events");
    assert_eq!(
        res2.out.state_changes.ledger_changes.0.get(&addr).unwrap(),
        &SetUpdateOrDelete::Update(LedgerEntryUpdate {
            balance: massa_ledger_exports::SetOrKeep::Set(Amount::from_str("50").unwrap()),
            bytecode: massa_ledger_exports::SetOrKeep::Keep,
            datastore: BTreeMap::new()
        })
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
fn test_nested_call_gas_usage() {
    // setup the period duration
    let exec_cfg = ExecutionConfig::default();
    let finalized_waitpoint = WaitPoint::new();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(&mut foreign_controllers.selector_controller);

    foreign_controllers
        .ledger_controller
        .set_expectations(|ledger_controller| {
            ledger_controller
                .expect_get_balance()
                .returning(move |_| Some(Amount::from_str("100").unwrap()));

            ledger_controller
                .expect_entry_exists()
                .times(2)
                .returning(move |_| false);

            ledger_controller
                .expect_entry_exists()
                .times(1)
                .returning(move |_| true);
        });
    let saved_bytecode = expect_finalize_deploy_and_call_blocks(
        Slot::new(1, 0),
        Some(Slot::new(1, 1)),
        finalized_waitpoint.get_trigger_handle(),
        &mut foreign_controllers.final_state,
    );
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        Some(saved_bytecode),
        None,
        None,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);

    // load bytecodes
    universe.deploy_bytecode_block(
        &KeyPair::from_str(TEST_SK_1).unwrap(),
        Slot::new(1, 0),
        include_bytes!("./wasm/nested_call.wasm"),
        include_bytes!("./wasm/test.wasm"),
    );
    finalized_waitpoint.wait();
    let address = universe.get_address_sc_deployed(Slot::new(1, 0));

    // Call the function test of the smart contract
    let operation = ExecutionTestUniverse::create_call_sc_operation(
        &KeyPair::from_str(TEST_SK_2).unwrap(),
        10000000,
        Amount::from_str("0").unwrap(),
        Amount::from_str("0").unwrap(),
        Address::from_str(&address).unwrap(),
        String::from("test"),
        address.as_bytes().to_vec(),
    )
    .unwrap();
    universe.call_sc_block(
        &KeyPair::from_str(TEST_SK_2).unwrap(),
        Slot::new(1, 1),
        operation,
    );
    finalized_waitpoint.wait();

    // Get the events that give us the gas usage (refer to source in ts) without fetching the first slot because it emit a event with an address.
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter {
            start: Some(Slot::new(1, 1)),
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
    let exec_cfg = ExecutionConfig::default();
    let finalized_waitpoint = WaitPoint::new();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();

    foreign_controllers
        .ledger_controller
        .set_expectations(|ledger_controller| {
            ledger_controller
                .expect_get_balance()
                .returning(move |_| Some(Amount::from_str("100").unwrap()));

            ledger_controller
                .expect_entry_exists()
                .times(2)
                .returning(move |_| false);

            ledger_controller
                .expect_entry_exists()
                .times(1)
                .returning(move |_| true);
        });

    selector_boilerplate(&mut foreign_controllers.selector_controller);
    let saved_bytecode = expect_finalize_deploy_and_call_blocks(
        Slot::new(1, 0),
        Some(Slot::new(1, 1)),
        finalized_waitpoint.get_trigger_handle(),
        &mut foreign_controllers.final_state,
    );
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        Some(saved_bytecode),
        None,
        None,
    );
    foreign_controllers
        .final_state
        .write()
        .expect_get_ops_exec_status()
        .returning(move |_| vec![Some(true)]);

    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);

    // load bytecodes
    universe.deploy_bytecode_block(
        &KeyPair::from_str(TEST_SK_1).unwrap(),
        Slot::new(1, 0),
        include_bytes!("./wasm/get_call_coins_main.wasm"),
        include_bytes!("./wasm/get_call_coins_test.wasm"),
    );
    finalized_waitpoint.wait();
    let address = universe.get_address_sc_deployed(Slot::new(1, 0));

    // Call the function test of the smart contract
    let coins_sent = Amount::from_str("10").unwrap();
    let operation = ExecutionTestUniverse::create_call_sc_operation(
        &KeyPair::from_str(TEST_SK_2).unwrap(),
        10000000,
        Amount::from_str("0").unwrap(),
        coins_sent,
        Address::from_str(&address).unwrap(),
        String::from("test"),
        address.as_bytes().to_vec(),
    )
    .unwrap();
    universe.call_sc_block(
        &KeyPair::from_str(TEST_SK_2).unwrap(),
        Slot::new(1, 1),
        operation.clone(),
    );
    finalized_waitpoint.wait();
    // Get the events that give us the gas usage (refer to source in ts) without fetching the first slot because it emit a event with an address.
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter {
            start: Some(Slot::new(1, 1)),
            ..Default::default()
        });
    assert!(events[0].data.contains(&format!(
        "tokens sent to the SC during the call : {}",
        coins_sent.to_raw()
    )));
    let (op_candidate, op_final) = universe
        .module_controller
        .get_ops_exec_status(&[operation.id])[0];

    // match the events
    assert!(
        op_candidate == Some(true) && op_final == Some(true),
        "Expected operation not found or not successfully executed"
    );
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
    let exec_cfg = ExecutionConfig::default();
    let finalized_waitpoint = WaitPoint::new();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    // TODO: add some context for this override
    foreign_controllers
        .selector_controller
        .set_expectations(|selector_controller| {
            selector_controller
                .expect_get_producer()
                .returning(move |_| {
                    Ok(Address::from_public_key(
                        &KeyPair::from_str(TEST_SK_2).unwrap().get_public_key(),
                    ))
                });
        });

    foreign_controllers
        .ledger_controller
        .set_expectations(|ledger_controller| {
            ledger_controller
                .expect_get_balance()
                .returning(move |_| Some(Amount::from_str("100").unwrap()));

            ledger_controller
                .expect_entry_exists()
                .times(2)
                .returning(move |_| false);

            ledger_controller
                .expect_entry_exists()
                .returning(move |_| true);
        });
    let saved_bytecode = Arc::new(RwLock::new(None));
    let saved_bytecode_edit = saved_bytecode.clone();
    let finalized_waitpoint_trigger_handle = finalized_waitpoint.get_trigger_handle();

    let destination = match *CHAINID {
        77 => Address::from_str("AS12jc7fTsSKwQ9hSk97C3iMNgNT1XrrD6MjSJRJZ4NE53YgQ4kFV").unwrap(),
        77658366 => {
            Address::from_str("AS12DSPbsNvvdP1ScCivmKpbQfcJJ3tCQFkNb8ewkRuNjsgoL2AeQ").unwrap()
        }
        77658377 => {
            Address::from_str("AS127QtY6Hzm6BnJc9wqCBfPNvEH9fKer3LiMNNQmcX3MzLwCL6G6").unwrap()
        }
        _ => panic!("CHAINID not supported"),
    };

    // Expected message from SC: send_message.ts (see massa unit tests src repo)
    let message = AsyncMessage {
        emission_slot: Slot {
            period: 1,
            thread: 0,
        },
        emission_index: 0,
        sender: Address::from_str("AU1TyzwHarZMQSVJgxku8co7xjrRLnH74nFbNpoqNd98YhJkWgi").unwrap(),
        // Note: generated address (from send_message.ts createSC call)
        //       this can changes when modification to the final state are done (see create_new_sc_address function)
        destination,
        function: String::from("receive"),
        // value from SC: send_message.ts
        max_gas: 3000000,
        fee: Amount::from_raw(1),
        coins: Amount::from_raw(100),
        validity_start: Slot {
            period: 1,
            thread: 1,
        },
        validity_end: Slot {
            period: 20,
            thread: 20,
        },
        function_params: vec![42, 42, 42, 42],
        trigger: None,
        can_be_executed: true,
    };
    let message_cloned = message.clone();
    foreign_controllers
        .final_state
        .write()
        .expect_finalize()
        .times(1)
        .with(predicate::eq(Slot::new(1, 0)), predicate::always())
        .returning(move |_, changes| {
            {
                let mut saved_bytecode = saved_bytecode_edit.write();
                *saved_bytecode = Some(changes.ledger_changes.get_bytecode_updates()[0].clone());
            }
            assert_eq!(changes.async_pool_changes.0.len(), 1);
            println!("changes: {:?}", changes.async_pool_changes.0);
            assert_eq!(
                changes.async_pool_changes.0.first_key_value().unwrap().1,
                &massa_ledger_exports::SetUpdateOrDelete::Set(message_cloned.clone())
            );
            assert_eq!(
                changes.async_pool_changes.0.first_key_value().unwrap().0,
                &message_cloned.compute_id()
            );
            finalized_waitpoint_trigger_handle.trigger();
        });

    let finalized_waitpoint_trigger_handle2 = finalized_waitpoint.get_trigger_handle();
    foreign_controllers
        .final_state
        .write()
        .expect_finalize()
        .times(1)
        .with(predicate::eq(Slot::new(1, 1)), predicate::always())
        .returning(move |_, changes| {
            match changes.ledger_changes.0.get(&destination).unwrap() {
                // sc has received the coins (0.0000001)
                SetUpdateOrDelete::Update(change_sc_update) => {
                    assert_eq!(
                        change_sc_update.balance,
                        SetOrKeep::Set(Amount::from_str("100.0000001").unwrap())
                    );
                }
                _ => panic!("wrong change type"),
            }

            match changes.async_pool_changes.0.first_key_value().unwrap().1 {
                SetUpdateOrDelete::Delete => {
                    // msg was deleted
                }
                _ => panic!("wrong change type"),
            }

            finalized_waitpoint_trigger_handle2.trigger();
        });

    let mut async_pool = AsyncPool::new(AsyncPoolConfig::default(), foreign_controllers.db.clone());
    let mut changes = BTreeMap::default();
    changes.insert(
        (
            Reverse(Ratio::new(1, 100000)),
            Slot {
                period: 1,
                thread: 0,
            },
            0,
        ),
        massa_ledger_exports::SetUpdateOrDelete::Set(message),
    );
    let mut db_batch = DBBatch::default();
    async_pool.apply_changes_to_batch(&AsyncPoolChanges(changes), &mut db_batch);
    foreign_controllers
        .db
        .write()
        .write_batch(db_batch, DBBatch::default(), Some(Slot::new(1, 0)));
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        Some(saved_bytecode),
        Some(async_pool),
        None,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());

    // load bytecodes
    universe.deploy_bytecode_block(
        &KeyPair::from_str(TEST_SK_1).unwrap(),
        Slot::new(1, 0),
        include_bytes!("./wasm/send_message.wasm"),
        include_bytes!("./wasm/receive_message.wasm"),
    );
    finalized_waitpoint.wait();

    let keypair = KeyPair::from_str(TEST_SK_2).unwrap();
    let block =
        ExecutionTestUniverse::create_block(&keypair, Slot::new(1, 1), vec![], vec![], vec![]);

    universe.send_and_finalize(&keypair, block);
    finalized_waitpoint.wait();
    // retrieve events emitted by smart contracts
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter {
            start: Some(Slot::new(1, 1)),
            end: Some(Slot::new(20, 1)),
            ..Default::default()
        });
    // match the events
    assert!(events.len() == 1, "One event was expected");
    assert_eq!(events[0].data, "message correctly received: 42,42,42,42");
}

#[test]
fn send_and_receive_async_message_expired() {
    let exec_cfg = ExecutionConfig::default();
    let finalized_waitpoint = WaitPoint::new();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    foreign_controllers
        .selector_controller
        .set_expectations(|selector_controller| {
            selector_controller
                .expect_get_producer()
                .returning(move |_| {
                    Ok(Address::from_public_key(
                        &KeyPair::from_str(TEST_SK_1).unwrap().get_public_key(),
                    ))
                });
        });

    foreign_controllers
        .ledger_controller
        .set_expectations(|ledger_controller| {
            ledger_controller
                .expect_get_balance()
                .returning(move |_| Some(Amount::from_str("100").unwrap()));

            ledger_controller
                .expect_entry_exists()
                .times(2)
                .returning(move |_| false);

            ledger_controller
                .expect_entry_exists()
                .returning(move |_| true);
        });
    let saved_bytecode = Arc::new(RwLock::new(None));
    let finalized_waitpoint_trigger_handle = finalized_waitpoint.get_trigger_handle();

    // Expected message from SC: send_message.ts (see massa unit tests src repo)
    foreign_controllers
        .final_state
        .write()
        .expect_finalize()
        .times(1)
        .with(predicate::eq(Slot::new(1, 0)), predicate::always())
        .returning(move |_, changes| {
            //println!("changes S (1 0): {:?}", changes);
            assert_eq!(
                changes.async_pool_changes,
                AsyncPoolChanges(BTreeMap::new())
            );
            finalized_waitpoint_trigger_handle.trigger();
        });

    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        Some(saved_bytecode),
        None,
        None,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());

    // load bytecodes
    universe.deploy_bytecode_block(
        &KeyPair::from_str(TEST_SK_1).unwrap(),
        Slot::new(1, 0),
        include_bytes!("./wasm/send_message_expired.wasm"),
        include_bytes!("./wasm/receive_message.wasm"),
    );
    println!("waiting for finalized");
    finalized_waitpoint.wait();

    // retrieve events emitted by smart contracts
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter {
            start: Some(Slot::new(1, 0)),
            end: Some(Slot::new(1, 1)),
            ..Default::default()
        });
    // match the events
    assert!(events.len() == 1, "One event was expected");
    assert!(events[0]
        .data
        .contains("validity end is earlier than the validity start"));
}

#[test]
fn send_and_receive_async_message_expired_2() {
    let exec_cfg = ExecutionConfig::default();
    let finalized_waitpoint = WaitPoint::new();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    foreign_controllers
        .selector_controller
        .set_expectations(|selector_controller| {
            selector_controller
                .expect_get_producer()
                .returning(move |_| {
                    Ok(Address::from_public_key(
                        &KeyPair::from_str(TEST_SK_1).unwrap().get_public_key(),
                    ))
                });
        });

    foreign_controllers
        .ledger_controller
        .set_expectations(|ledger_controller| {
            ledger_controller
                .expect_get_balance()
                .returning(move |_| Some(Amount::from_str("100").unwrap()));

            ledger_controller
                .expect_entry_exists()
                .times(2)
                .returning(move |_| false);

            ledger_controller
                .expect_entry_exists()
                .returning(move |_| true);
        });
    let saved_bytecode = Arc::new(RwLock::new(None));
    let finalized_waitpoint_trigger_handle = finalized_waitpoint.get_trigger_handle();

    // Expected message from SC: send_message.ts (see massa unit tests src repo)
    foreign_controllers
        .final_state
        .write()
        .expect_finalize()
        .times(1)
        .with(predicate::eq(Slot::new(1, 0)), predicate::always())
        .returning(move |_, changes| {
            assert_eq!(
                changes.async_pool_changes,
                AsyncPoolChanges(BTreeMap::new())
            );
            finalized_waitpoint_trigger_handle.trigger();
        });

    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        Some(saved_bytecode),
        None,
        None,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());

    // load bytecodes
    universe.deploy_bytecode_block(
        &KeyPair::from_str(TEST_SK_1).unwrap(),
        Slot::new(1, 0),
        include_bytes!("./wasm/send_message_expired_2.wasm"),
        include_bytes!("./wasm/receive_message.wasm"),
    );
    println!("waiting for finalized");
    finalized_waitpoint.wait();

    // retrieve events emitted by smart contracts
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter {
            start: Some(Slot::new(1, 0)),
            end: Some(Slot::new(1, 1)),
            ..Default::default()
        });
    // match the events
    assert!(events.len() == 1, "One event was expected");
    assert!(events[0]
        .data
        .contains("validity end is earlier than the current slot"));
}

#[test]
fn send_and_receive_async_message_without_init_gas() {
    let mut exec_cfg = ExecutionConfig::default();
    exec_cfg.gas_costs.max_instance_cost = 4000000;

    let finalized_waitpoint = WaitPoint::new();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    foreign_controllers
        .selector_controller
        .set_expectations(|selector_controller| {
            selector_controller
                .expect_get_producer()
                .returning(move |_| {
                    Ok(Address::from_public_key(
                        &KeyPair::from_str(TEST_SK_1).unwrap().get_public_key(),
                    ))
                });
        });

    foreign_controllers
        .ledger_controller
        .set_expectations(|ledger_controller| {
            ledger_controller
                .expect_get_balance()
                .returning(move |_| Some(Amount::from_str("100").unwrap()));

            ledger_controller
                .expect_entry_exists()
                .times(2)
                .returning(move |_| false);

            ledger_controller
                .expect_entry_exists()
                .returning(move |_| true);
        });
    let saved_bytecode = Arc::new(RwLock::new(None));
    let finalized_waitpoint_trigger_handle = finalized_waitpoint.get_trigger_handle();

    // Expected message from SC: send_message.ts (see massa unit tests src repo)
    foreign_controllers
        .final_state
        .write()
        .expect_finalize()
        .times(1)
        .with(predicate::eq(Slot::new(1, 0)), predicate::always())
        .returning(move |_, changes| {
            assert_eq!(
                changes.async_pool_changes,
                AsyncPoolChanges(BTreeMap::new())
            );
            finalized_waitpoint_trigger_handle.trigger();
        });

    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        Some(saved_bytecode),
        None,
        None,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());

    // load bytecodes
    universe.deploy_bytecode_block(
        &KeyPair::from_str(TEST_SK_1).unwrap(),
        Slot::new(1, 0),
        include_bytes!("./wasm/send_message.wasm"),
        include_bytes!("./wasm/receive_message.wasm"),
    );
    println!("waiting for finalized");
    finalized_waitpoint.wait();

    // retrieve events emitted by smart contracts
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter {
            start: Some(Slot::new(1, 0)),
            end: Some(Slot::new(1, 1)),
            ..Default::default()
        });
    // match the events
    assert!(events.len() == 1, "One event was expected");
    assert!(events[0]
        .data
        .contains("max gas is lower than the minimum instance cost"));
}

#[test]
fn cancel_async_message() {
    let exec_cfg = ExecutionConfig::default();
    let finalized_waitpoint = WaitPoint::new();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    foreign_controllers
        .selector_controller
        .set_expectations(|selector_controller| {
            selector_controller
                .expect_get_producer()
                .returning(move |_| {
                    Ok(Address::from_public_key(
                        &KeyPair::from_str(TEST_SK_2).unwrap().get_public_key(),
                    ))
                });
        });
    let saved_bytecode = Arc::new(RwLock::new(None));
    let saved_bytecode_edit = saved_bytecode.clone();
    let finalized_waitpoint_trigger_handle = finalized_waitpoint.get_trigger_handle();
    let sender_addr =
        Address::from_str("AU1TyzwHarZMQSVJgxku8co7xjrRLnH74nFbNpoqNd98YhJkWgi").unwrap();
    let message = AsyncMessage {
        emission_slot: Slot {
            period: 1,
            thread: 0,
        },
        emission_index: 0,
        sender: sender_addr,
        destination: Address::from_str("AU12mzL2UWroPV7zzHpwHnnF74op9Gtw7H55fAmXMnCuVZTFSjZCA")
            .unwrap(),
        function: String::from("receive"),
        max_gas: 3000000,
        fee: Amount::from_raw(1),
        coins: Amount::from_raw(100),
        validity_start: Slot {
            period: 1,
            thread: 1,
        },
        validity_end: Slot {
            period: 20,
            thread: 20,
        },
        function_params: vec![42, 42, 42, 42],
        trigger: None,
        can_be_executed: true,
    };
    foreign_controllers
        .final_state
        .write()
        .expect_finalize()
        .times(1)
        .with(predicate::eq(Slot::new(1, 0)), predicate::always())
        .returning(move |_, changes| {
            {
                let mut saved_bytecode = saved_bytecode_edit.write();
                *saved_bytecode = Some(changes.ledger_changes.get_bytecode_updates()[0].clone());
            }
            assert_eq!(
                changes.ledger_changes.0.get(&sender_addr).unwrap(),
                &SetUpdateOrDelete::Update(LedgerEntryUpdate {
                    balance: massa_ledger_exports::SetOrKeep::Set(
                        Amount::from_str("100.670399899").unwrap()
                    ),
                    bytecode: massa_ledger_exports::SetOrKeep::Keep,
                    datastore: BTreeMap::new()
                })
            );

            finalized_waitpoint_trigger_handle.trigger();
        });

    let finalized_waitpoint_trigger_handle2 = finalized_waitpoint.get_trigger_handle();
    foreign_controllers
        .final_state
        .write()
        .expect_finalize()
        .times(1)
        .with(predicate::eq(Slot::new(1, 1)), predicate::always())
        .returning(move |_, changes| {
            match changes.ledger_changes.0.get(&sender_addr).unwrap() {
                // at slot (1,1) msg was canceled so sender has received the coins (0.0000001)
                // sender has received the coins (0.0000001)
                SetUpdateOrDelete::Update(change_sender_update) => {
                    assert_eq!(
                        change_sender_update.balance,
                        SetOrKeep::Set(Amount::from_str("100.0000001").unwrap())
                    );
                }
                _ => panic!("wrong change type"),
            }

            match changes.async_pool_changes.0.first_key_value().unwrap().1 {
                SetUpdateOrDelete::Delete => {
                    // msg was deleted
                }
                _ => panic!("wrong change type"),
            }

            finalized_waitpoint_trigger_handle2.trigger();
        });

    let mut async_pool = AsyncPool::new(AsyncPoolConfig::default(), foreign_controllers.db.clone());
    let mut changes = BTreeMap::default();
    changes.insert(
        (
            Reverse(Ratio::new(1, 100000)),
            Slot {
                period: 1,
                thread: 0,
            },
            0,
        ),
        massa_ledger_exports::SetUpdateOrDelete::Set(message),
    );
    let mut db_batch = DBBatch::default();
    async_pool.apply_changes_to_batch(&AsyncPoolChanges(changes), &mut db_batch);
    foreign_controllers
        .db
        .write()
        .write_batch(db_batch, DBBatch::default(), Some(Slot::new(1, 0)));
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        Some(saved_bytecode),
        Some(async_pool),
        None,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());

    // load bytecodes
    universe.deploy_bytecode_block(
        &KeyPair::from_str(TEST_SK_1).unwrap(),
        Slot::new(1, 0),
        include_bytes!("./wasm/send_message.wasm"),
        include_bytes!("./wasm/receive_message.wasm"),
    );
    finalized_waitpoint.wait();

    let keypair = KeyPair::from_str(TEST_SK_2).unwrap();
    let block =
        ExecutionTestUniverse::create_block(&keypair, Slot::new(1, 1), vec![], vec![], vec![]);

    universe.send_and_finalize(&keypair, block);
    finalized_waitpoint.wait();

    // Sleep to wait (1,1) candidate slot to be executed. We don't have a mock to waitpoint on or empty block
    std::thread::sleep(Duration::from_millis(exec_cfg.t0.as_millis()));
    // retrieve events emitted by smart contracts
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter {
            start: Some(Slot::new(1, 1)),
            end: Some(Slot::new(20, 1)),
            ..Default::default()
        });
    assert!(events[0].data.contains(" is not a smart contract address"));
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
    let exec_cfg = ExecutionConfig::default();
    let finalized_waitpoint = WaitPoint::new();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    expect_finalize_deploy_and_call_blocks(
        Slot::new(1, 0),
        None,
        finalized_waitpoint.get_trigger_handle(),
        &mut foreign_controllers.final_state,
    );
    expect_finalize_deploy_and_call_blocks(
        Slot::new(1, 1),
        None,
        finalized_waitpoint.get_trigger_handle(),
        &mut foreign_controllers.final_state,
    );

    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        None,
        None,
        None,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);
    // load bytecodes
    universe.deploy_bytecode_block(
        &KeyPair::from_str(TEST_SK_1).unwrap(),
        Slot::new(1, 0),
        include_bytes!("./wasm/local_execution.wasm"),
        include_bytes!("./wasm/local_function.wasm"),
    );
    finalized_waitpoint.wait();

    universe.deploy_bytecode_block(
        &KeyPair::from_str(TEST_SK_2).unwrap(),
        Slot::new(1, 1),
        include_bytes!("./wasm/local_call.wasm"),
        include_bytes!("./wasm/local_function.wasm"),
    );
    finalized_waitpoint.wait();

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
        Amount::from_str("90").unwrap() // start (100) - fee (10)
    );
    assert_eq!(events[1].context.call_stack.len(), 1);
    assert_eq!(
        events[1].context.call_stack.back().unwrap(),
        &Address::from_public_key(&KeyPair::from_str(TEST_SK_1).unwrap().get_public_key())
    );
    assert_eq!(events[2].data, "one local execution completed");
    let amount = Amount::from_raw(events[5].data.parse().unwrap());
    assert_eq!(Amount::from_str("89.6713").unwrap(), amount);
    assert_eq!(events[5].context.call_stack.len(), 1);
    assert_eq!(
        events[1].context.call_stack.back().unwrap(),
        &Address::from_public_key(&KeyPair::from_str(TEST_SK_1).unwrap().get_public_key())
    );
    assert_eq!(events[6].data, "one local call completed");
}

/// Context
///
/// Functional test for sc deployment utility functions, `functionExists` and `callerHasWriteAccess`
///
/// 1. a block is created with one ExecuteSC operation containing
///    a deployment sc as bytecode to execute and a deployed sc as an op datatsore entry
/// 2. store and set the block as final
/// 3. wait for execution
/// 4. retrieve events emitted by the initial an sub functions
/// 5. match events to make sure that `functionExists` and `callerHasWriteAccess` had the expected behaviour
#[test]
fn sc_deployment() {
    // setup the period duration
    let exec_cfg = ExecutionConfig::default();
    let finalized_waitpoint = WaitPoint::new();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    let saved_bytecode = expect_finalize_deploy_and_call_blocks(
        Slot::new(1, 0),
        None,
        finalized_waitpoint.get_trigger_handle(),
        &mut foreign_controllers.final_state,
    );
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        Some(saved_bytecode),
        None,
        None,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);
    // load bytecodes
    universe.deploy_bytecode_block(
        &KeyPair::from_str(TEST_SK_1).unwrap(),
        Slot::new(1, 0),
        include_bytes!("./wasm/deploy_sc.wasm"),
        include_bytes!("./wasm/init_sc.wasm"),
    );
    finalized_waitpoint.wait();
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
/// 5. We send a new operation with a smart contract that modify `test` datastore key and so doesn't trigger the message.
/// 6. We send a new operation with a smart contract that create `test2` datastore key and so trigger the message.
/// 7. once the execution period is over we stop the execution controller
/// 8. we retrieve the events emitted by smart contract
/// 9. `test` handler function should have emitted a second event
/// 10. we check if they are events
/// 11. if they are some, we verify that the data has the correct value
#[test]
fn send_and_receive_async_message_with_trigger() {
    // setup the period duration
    let exec_cfg = ExecutionConfig::default();
    let finalized_waitpoint = WaitPoint::new();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    expect_finalize_deploy_and_call_blocks(
        Slot::new(1, 0),
        None,
        finalized_waitpoint.get_trigger_handle(),
        &mut foreign_controllers.final_state,
    );
    expect_finalize_deploy_and_call_blocks(
        Slot::new(1, 1),
        None,
        finalized_waitpoint.get_trigger_handle(),
        &mut foreign_controllers.final_state,
    );
    expect_finalize_deploy_and_call_blocks(
        Slot::new(1, 2),
        None,
        finalized_waitpoint.get_trigger_handle(),
        &mut foreign_controllers.final_state,
    );
    foreign_controllers
        .ledger_controller
        .set_expectations(|ledger_controller| {
            ledger_controller
                .expect_get_data_entry()
                .returning(move |_, _| None);
        });
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        None,
        None,
        None,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());
    // load bytecode
    // you can check the source code of the following wasm file in massa-unit-tests-src
    // TODO: Fix key opData in massa-unit-test-src to be standard with the others
    let mut datastore = BTreeMap::new();
    let key = unsafe {
        String::from("smart-contract")
            .encode_utf16()
            .collect::<Vec<u16>>()
            .align_to::<u8>()
            .1
            .to_vec()
    };
    datastore.insert(
        key,
        include_bytes!("./wasm/send_message_condition.wasm").to_vec(),
    );

    // create the block containing the smart contract execution operation
    let operation = ExecutionTestUniverse::create_execute_sc_operation(
        &KeyPair::from_str(TEST_SK_1).unwrap(),
        include_bytes!("./wasm/send_message_deploy_condition.wasm"),
        datastore,
    )
    .unwrap();
    universe.storage.store_operations(vec![operation.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &KeyPair::from_str(TEST_SK_1).unwrap(),
        Slot::new(1, 0),
        vec![operation],
        vec![],
        vec![],
    );
    universe.storage.store_block(block.clone());
    universe.send_and_finalize(&KeyPair::from_str(TEST_SK_1).unwrap(), block);
    finalized_waitpoint.wait();
    // retrieve events emitted by smart contracts
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter {
            ..Default::default()
        });

    // match the events
    assert_eq!(events.len(), 2, "2 events were expected");
    assert_eq!(events[0].data, "Triggered");

    // load bytecode
    // you can check the source code of the following wasm file in massa-unit-tests-src
    universe.deploy_bytecode_block(
        &KeyPair::from_str(TEST_SK_2).unwrap(),
        Slot::new(1, 1),
        include_bytes!("./wasm/send_message_wrong_trigger.wasm"),
        //unused in this case
        include_bytes!("./wasm/send_message_condition.wasm"),
    );
    finalized_waitpoint.wait();

    // retrieve events emitted by smart contracts (no new because we made the wrong trigger)
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter {
            ..Default::default()
        });

    // match the events
    assert!(events.len() == 3, "3 events were expected");

    // load bytecode
    // you can check the source code of the following wasm file in massa-unit-tests-src
    // This line execute the smart contract that will modify the data entry and then trigger the SC.
    universe.deploy_bytecode_block(
        &KeyPair::from_str(TEST_SK_3).unwrap(),
        Slot::new(1, 2),
        include_bytes!("./wasm/send_message_trigger.wasm"),
        //unused in this case
        include_bytes!("./wasm/send_message_condition.wasm"),
    );
    finalized_waitpoint.wait();

    // Pass few slots as candidate (no way for now to catch this in a mock)
    std::thread::sleep(Duration::from_millis(
        exec_cfg.t0.as_millis().checked_div(2).unwrap(),
    ));
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
    let exec_cfg = ExecutionConfig::default();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    let finalized_waitpoint = WaitPoint::new();
    let finalized_waitpoint_trigger_handle = finalized_waitpoint.get_trigger_handle();
    let recipient_address =
        Address::from_public_key(&KeyPair::generate(0).unwrap().get_public_key());
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        None,
        None,
        None,
    );
    foreign_controllers
        .final_state
        .write()
        .expect_finalize()
        .times(1)
        .with(predicate::eq(Slot::new(1, 0)), predicate::always())
        .returning(move |_, changes| {
            // 190 because 100 in the get_balance in the `final_state_boilerplate` and 90 from the transfer.
            assert_eq!(
                changes
                    .ledger_changes
                    .get_balance_or_else(&recipient_address, || None),
                Some(Amount::from_str("190").unwrap())
            );
            // 1.02 for the block rewards
            assert_eq!(
                changes.ledger_changes.get_balance_or_else(
                    &Address::from_public_key(
                        &KeyPair::from_str(TEST_SK_1).unwrap().get_public_key()
                    ),
                    || None
                ),
                Some(
                    exec_cfg
                        .block_reward
                        .saturating_add(Amount::from_str("10").unwrap()) // add 10 fee
                )
            );
            finalized_waitpoint_trigger_handle.trigger();
        });
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());
    // create the operation
    let operation = Operation::new_verifiable(
        Operation {
            fee: Amount::from_str("10").unwrap(),
            expire_period: 10,
            op: OperationType::Transaction {
                recipient_address,
                amount: Amount::from_str("90").unwrap(),
            },
        },
        OperationSerializer::new(),
        &KeyPair::from_str(TEST_SK_1).unwrap(),
        *CHAINID,
    )
    .unwrap();
    // create the block containing the transaction operation
    universe.storage.store_operations(vec![operation.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &KeyPair::from_str(TEST_SK_1).unwrap(),
        Slot::new(1, 0),
        vec![operation],
        vec![],
        vec![],
    );
    // store the block in storage
    universe.send_and_finalize(&KeyPair::from_str(TEST_SK_1).unwrap(), block);
    finalized_waitpoint.wait();
}

#[test]
fn roll_buy() {
    // setup
    let exec_cfg = ExecutionConfig::default();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    let finalized_waitpoint = WaitPoint::new();
    let finalized_waitpoint_trigger_handle = finalized_waitpoint.get_trigger_handle();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
    let address = Address::from_public_key(&keypair.get_public_key());
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        None,
        None,
        None,
    );
    foreign_controllers
        .final_state
        .write()
        .expect_finalize()
        .times(1)
        .with(predicate::eq(Slot::new(1, 0)), predicate::always())
        .returning(move |_, changes| {
            assert_eq!(changes.pos_changes.roll_changes.len(), 1);
            // 100 base + 1 bought
            assert_eq!(changes.pos_changes.roll_changes.get(&address), Some(&101));

            // address has 100 coins before buying roll
            // -> (100 (balance) - 100 (roll price)) + 1.02 (block reward)
            assert_eq!(
                changes.ledger_changes.0.get(&address).unwrap(),
                &SetUpdateOrDelete::Update(LedgerEntryUpdate {
                    balance: massa_ledger_exports::SetOrKeep::Set(exec_cfg.block_reward),
                    bytecode: massa_ledger_exports::SetOrKeep::Keep,
                    datastore: BTreeMap::new()
                })
            );

            finalized_waitpoint_trigger_handle.trigger();
        });
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());
    // create the operation
    let operation = Operation::new_verifiable(
        Operation {
            fee: Amount::zero(),
            expire_period: 10,
            op: OperationType::RollBuy { roll_count: 1 },
        },
        OperationSerializer::new(),
        &KeyPair::from_str(TEST_SK_1).unwrap(),
        *CHAINID,
    )
    .unwrap();
    // create the block containing the roll buy operation
    universe.storage.store_operations(vec![operation.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &KeyPair::from_str(TEST_SK_1).unwrap(),
        Slot::new(1, 0),
        vec![operation],
        vec![],
        vec![],
    );
    // set our block as a final block so the purchase is processed
    universe.send_and_finalize(&KeyPair::from_str(TEST_SK_1).unwrap(), block);
    finalized_waitpoint.wait();
}

#[test]
fn roll_sell() {
    // setup
    let exec_cfg = ExecutionConfig {
        thread_count: 2,
        periods_per_cycle: 2,
        last_start_period: 2,
        max_miss_ratio: Ratio::new(1, 1),
        ..Default::default()
    };
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    let finalized_waitpoint = WaitPoint::new();
    let finalized_waitpoint_trigger_handle = finalized_waitpoint.get_trigger_handle();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
    let address = Address::from_public_key(&keypair.get_public_key());
    let (rolls_path, _) = get_initials();
    let mut batch = DBBatch::new();
    let initial_deferred_credits = Amount::from_str("100").unwrap();
    let mut pos_final_state = PoSFinalState::new(
        PoSConfig::default(),
        "",
        &rolls_path.into_temp_path().to_path_buf(),
        foreign_controllers.selector_controller.clone(),
        foreign_controllers.db.clone(),
    )
    .unwrap();
    pos_final_state.create_initial_cycle(&mut batch);
    pos_final_state.put_deferred_credits_entry(
        &Slot::new(1, 0),
        &address,
        &initial_deferred_credits,
        &mut batch,
    );

    foreign_controllers
        .db
        .write()
        .write_batch(batch, Default::default(), None);
    pos_final_state.recompute_pos_state_caches();
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        None,
        None,
        Some(pos_final_state),
    );
    foreign_controllers
        .final_state
        .write()
        .expect_finalize()
        .times(1)
        .with(predicate::eq(Slot::new(3, 0)), predicate::always())
        .returning(move |_, changes| {
            let amount = changes
                .ledger_changes
                .get_balance_or_else(&address, || None)
                .unwrap();
            assert_eq!(
                amount,
                // 100 from the boilerplate
                Amount::from_mantissa_scale(100, 0)
                    .unwrap()
                    // + deferred credits set above
                    .saturating_add(initial_deferred_credits)
                    // + block rewards
                    .saturating_add(exec_cfg.block_reward)
            );
            let deferred_credits = changes
                .pos_changes
                .deferred_credits
                .get_address_credits_for_slot(&address, &Slot::new(9, 1))
                .unwrap();
            assert_eq!(
                deferred_credits,
                Amount::from_mantissa_scale(1100, 0).unwrap()
            );
            finalized_waitpoint_trigger_handle.trigger();
        });
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());
    // create the operations
    let roll_sell_1 = 10;
    let roll_sell_2 = 1;

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
        *CHAINID,
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
        *CHAINID,
    )
    .unwrap();
    // create the block containing the roll buy operation
    universe
        .storage
        .store_operations(vec![operation1.clone(), operation2.clone()]);
    let block = ExecutionTestUniverse::create_block(
        &keypair,
        Slot::new(3, 0),
        vec![operation1, operation2],
        vec![],
        vec![],
    );
    // set our block as a final block so the purchase is processed
    universe.send_and_finalize(&keypair, block);
    finalized_waitpoint.wait();
}

#[test]
fn auto_sell_on_missed_blocks() {
    // setup
    let exec_cfg = ExecutionConfig {
        thread_count: 2,
        periods_per_cycle: 2,
        max_miss_ratio: Ratio::new(0, 1),
        ..Default::default()
    };
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    let finalized_waitpoint = WaitPoint::new();
    let finalized_waitpoint_trigger_handle = finalized_waitpoint.get_trigger_handle();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
    let address = Address::from_public_key(&keypair.get_public_key());
    let (rolls_path, _) = get_initials();
    let mut batch = DBBatch::new();
    let initial_deferred_credits = Amount::from_str("100").unwrap();
    let mut pos_final_state = PoSFinalState::new(
        PoSConfig {
            thread_count: 2,
            periods_per_cycle: 2,
            ..PoSConfig::default()
        },
        "",
        &rolls_path.into_temp_path().to_path_buf(),
        foreign_controllers.selector_controller.clone(),
        foreign_controllers.db.clone(),
    )
    .unwrap();
    pos_final_state.create_initial_cycle(&mut batch);
    pos_final_state.put_deferred_credits_entry(
        &Slot::new(1, 0),
        &address,
        &initial_deferred_credits,
        &mut batch,
    );

    pos_final_state.put_deferred_credits_entry(
        &Slot::new(7, 1),
        &address,
        &initial_deferred_credits,
        &mut batch,
    );

    let mut prod_stats = PreHashMap::default();
    prod_stats.insert(
        address,
        ProductionStats {
            block_success_count: 0,
            block_failure_count: 1,
        },
    );

    let rolls_cycle_0 = pos_final_state.get_all_roll_counts(0);
    let cycle_info_0 = CycleInfo::new(
        0,
        false,
        rolls_cycle_0.clone(),
        Default::default(),
        prod_stats.clone(),
    );
    pos_final_state.put_new_cycle_info(&cycle_info_0, &mut batch);

    foreign_controllers
        .db
        .write()
        .write_batch(batch, Default::default(), None);
    pos_final_state.recompute_pos_state_caches();

    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        None,
        None,
        Some(pos_final_state.clone()),
    );

    foreign_controllers
        .final_state
        .write()
        .expect_finalize()
        .times(1)
        .with(predicate::eq(Slot::new(1, 0)), predicate::always())
        .returning(move |_, _changes| {});

    foreign_controllers
        .final_state
        .write()
        .expect_finalize()
        .with(predicate::eq(Slot::new(1, 1)), predicate::always())
        .returning(move |_, changes| {
            println!("changes: {:?}", changes);
            let deferred_credits = changes
                .pos_changes
                .deferred_credits
                .get_address_credits_for_slot(&address, &Slot::new(7, 1))
                .unwrap();
            assert_eq!(
                deferred_credits,
                Amount::from_mantissa_scale(10100, 0).unwrap()
            );
            finalized_waitpoint_trigger_handle.trigger();
        });

    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());

    let block =
        ExecutionTestUniverse::create_block(&keypair, Slot::new(1, 0), vec![], vec![], vec![]);
    // set our block as a final block so the purchase is processed
    universe.send_and_finalize(&keypair, block);
    let block =
        ExecutionTestUniverse::create_block(&keypair, Slot::new(1, 1), vec![], vec![], vec![]);
    universe.send_and_finalize(&keypair, block);
    finalized_waitpoint.wait();
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
        periods_per_cycle: 2,
        thread_count: 2,
        last_start_period: 0,
        roll_count_to_slash_on_denunciation: 3, // Set to 3 to check if config is taken into account
        max_miss_ratio: Ratio::new(1, 1),
        ..Default::default()
    };
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
    let address = Address::from_public_key(&keypair.get_public_key());
    let waitpoint = WaitPoint::new();
    let waitpoint_trigger_handle = waitpoint.get_trigger_handle();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    foreign_controllers
        .selector_controller
        .set_expectations(|selector_controller| {
            selector_controller
                .expect_get_selection()
                .returning(move |_| {
                    Ok(Selection {
                        endorsements: vec![address; ENDORSEMENT_COUNT as usize],
                        producer: address,
                    })
                });
        });
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    foreign_controllers
        .final_state
        .write()
        .expect_finalize()
        .returning(move |_, changes| {
            let rolls = changes.pos_changes.roll_changes.get(&address).unwrap();
            // 97 sold and 3 slashed
            assert_eq!(rolls, &0);
            let deferred_credits = changes
                .pos_changes
                .deferred_credits
                .get_address_credits_for_slot(&address, &Slot::new(7, 1))
                .unwrap();
            // Only the 97 sold
            assert_eq!(
                deferred_credits,
                Amount::from_mantissa_scale(9700, 0).unwrap()
            );
            let balance = changes
                .ledger_changes
                .get_balance_or_else(&address, || None)
                .unwrap();
            // 100 base + reward of the 3 slash (50%) + block reward
            assert_eq!(
                balance,
                Amount::from_mantissa_scale(100, 0)
                    .unwrap()
                    .saturating_add(exec_cfg.block_reward)
                    .saturating_add(Amount::from_mantissa_scale(150, 0).unwrap())
            );
            waitpoint_trigger_handle.trigger()
        });
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        None,
        None,
        None,
    );

    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());

    // create operation 1
    let operation1 = Operation::new_verifiable(
        Operation {
            fee: Amount::zero(),
            expire_period: 6,
            op: OperationType::RollSell { roll_count: 97 },
        },
        OperationSerializer::new(),
        &keypair,
        *CHAINID,
    )
    .unwrap();

    // create a denunciation
    let (_slot, _keypair, s_endorsement_1, s_endorsement_2, _) =
        gen_endorsements_for_denunciation(Some(Slot::new(1, 0)), Some(keypair.clone()));
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
        &keypair,
        Slot::new(1, 0),
        vec![operation1],
        vec![],
        vec![denunciation, denunciation_2],
    );
    universe.send_and_finalize(&keypair, block);
    waitpoint.wait();
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
        periods_per_cycle: 2,
        thread_count: 2,
        last_start_period: 0,
        roll_count_to_slash_on_denunciation: 4, // Set to 4 to check if config is taken into account
        max_miss_ratio: Ratio::new(1, 1),
        ..Default::default()
    };
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
    let address = Address::from_public_key(&keypair.get_public_key());
    let waitpoint = WaitPoint::new();
    let waitpoint_trigger_handle = waitpoint.get_trigger_handle();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    foreign_controllers
        .selector_controller
        .set_expectations(|selector_controller| {
            selector_controller
                .expect_get_selection()
                .returning(move |_| {
                    Ok(Selection {
                        endorsements: vec![address; ENDORSEMENT_COUNT as usize],
                        producer: address,
                    })
                });
        });
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    foreign_controllers
        .final_state
        .write()
        .expect_finalize()
        .returning(move |_, changes| {
            let rolls = changes.pos_changes.roll_changes.get(&address).unwrap();
            // 100 sold
            assert_eq!(rolls, &0);
            let deferred_credits = changes
                .pos_changes
                .deferred_credits
                .get_address_credits_for_slot(&address, &Slot::new(7, 1))
                .unwrap();
            // Only amount of 96 sold as 4 are slashed
            assert_eq!(
                deferred_credits,
                Amount::from_mantissa_scale(9600, 0).unwrap()
            );
            let balance = changes
                .ledger_changes
                .get_balance_or_else(&address, || None)
                .unwrap();
            // 100 base + reward of the 4 slash (50%) + block reward
            assert_eq!(
                balance,
                Amount::from_mantissa_scale(100, 0)
                    .unwrap()
                    .saturating_add(exec_cfg.block_reward)
                    .saturating_add(Amount::from_mantissa_scale(200, 0).unwrap())
            );
            waitpoint_trigger_handle.trigger()
        });
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        None,
        None,
        None,
    );

    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());

    // create operation 1
    let operation1 = Operation::new_verifiable(
        Operation {
            fee: Amount::zero(),
            expire_period: 6,
            op: OperationType::RollSell { roll_count: 100 },
        },
        OperationSerializer::new(),
        &keypair,
        *CHAINID,
    )
    .unwrap();

    // create a denunciation
    let (_slot, _keypair, s_endorsement_1, s_endorsement_2, _) =
        gen_endorsements_for_denunciation(Some(Slot::new(1, 0)), Some(keypair.clone()));
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
        &keypair,
        Slot::new(1, 0),
        vec![operation1],
        vec![],
        vec![denunciation, denunciation_2],
    );
    universe.send_and_finalize(&keypair, block);
    waitpoint.wait();
}

#[test]
fn sc_execution_error() {
    let exec_cfg = ExecutionConfig::default();
    let finalized_waitpoint = WaitPoint::new();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    expect_finalize_deploy_and_call_blocks(
        Slot::new(1, 0),
        None,
        finalized_waitpoint.get_trigger_handle(),
        &mut foreign_controllers.final_state,
    );
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        None,
        None,
        None,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);
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
    universe.send_and_finalize(&keypair, block);
    finalized_waitpoint.wait();
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
    let exec_cfg = ExecutionConfig::default();
    let finalized_waitpoint = WaitPoint::new();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    let saved_bytecode = expect_finalize_deploy_and_call_blocks(
        Slot::new(1, 0),
        None,
        finalized_waitpoint.get_trigger_handle(),
        &mut foreign_controllers.final_state,
    );
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        Some(saved_bytecode),
        None,
        None,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);
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
    universe.send_and_finalize(&keypair, block);
    finalized_waitpoint.wait();
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
    let exec_cfg = ExecutionConfig::default();
    let finalized_waitpoint = WaitPoint::new();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    let saved_bytecode = expect_finalize_deploy_and_call_blocks(
        Slot::new(1, 0),
        None,
        finalized_waitpoint.get_trigger_handle(),
        &mut foreign_controllers.final_state,
    );
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        Some(saved_bytecode),
        None,
        None,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);
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
    universe.send_and_finalize(&keypair, block);
    finalized_waitpoint.wait();
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
    let exec_cfg = ExecutionConfig::default();
    let finalized_waitpoint = WaitPoint::new();
    let finalized_waitpoint_trigger_handle = finalized_waitpoint.get_trigger_handle();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
    let addr = Address::from_public_key(&keypair.get_public_key());
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
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    foreign_controllers
        .ledger_controller
        .set_expectations(|ledger_controller| {
            ledger_controller
                .expect_get_data_entry()
                .returning(move |_, _| None);
            ledger_controller
                .expect_get_datastore_keys()
                .returning(move |_, _| None);
            ledger_controller
                .expect_get_bytecode()
                .returning(move |_| None);
        });
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        None,
        None,
        None,
    );
    foreign_controllers
        .final_state
        .write()
        .expect_get_fingerprint()
        .returning(move || Hash::compute_from(b""));
    foreign_controllers
        .final_state
        .write()
        .expect_finalize()
        .times(1)
        .with(predicate::eq(Slot::new(1, 0)), predicate::always())
        .returning(move |_, changes| {
            let key_len = (key_a.len() + key_b.len()) as u64;
            let value_len = ([21, 0, 49].len() + [5, 12, 241].len()) as u64;
            let amount = changes
                .ledger_changes
                .get_balance_or_else(&addr, || None)
                .unwrap();
            assert_eq!(
                amount,
                // Base from the boilerplate
                Amount::from_str("100")
                    .unwrap()
                    .saturating_sub(Amount::const_init(10, 0))
                    .saturating_add(exec_cfg.block_reward)
                    // Gas fee
                    .saturating_add(Amount::from_str("10").unwrap())
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
            finalized_waitpoint_trigger_handle.trigger();
        });
    foreign_controllers
        .final_state
        .write()
        .expect_get_ops_exec_status()
        .returning(move |_| vec![Some(true)]);
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
        &keypair,
        Slot::new(1, 0),
        vec![operation.clone()],
        vec![],
        vec![],
    );
    universe.send_and_finalize(&keypair, block);
    finalized_waitpoint.wait();

    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter::default());
    // match the events
    assert_eq!(events.len(), 4, "Got {} events, expected 4", events.len());

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

    let key_a: Vec<u8> = [1, 0, 4, 255].to_vec();

    universe
        .module_controller
        .query_state(ExecutionQueryRequest {
            requests: vec![
                ExecutionQueryRequestItem::AddressExistsCandidate(addr),
                ExecutionQueryRequestItem::AddressExistsFinal(addr),
                ExecutionQueryRequestItem::AddressBalanceCandidate(addr),
                ExecutionQueryRequestItem::AddressBalanceFinal(addr),
                ExecutionQueryRequestItem::AddressBytecodeCandidate(addr),
                ExecutionQueryRequestItem::AddressBytecodeFinal(addr),
                ExecutionQueryRequestItem::AddressDatastoreKeysCandidate {
                    addr,
                    prefix: vec![],
                },
                ExecutionQueryRequestItem::AddressDatastoreKeysFinal {
                    addr,
                    prefix: vec![],
                },
                ExecutionQueryRequestItem::AddressDatastoreValueCandidate {
                    addr,
                    key: key_a.clone(),
                },
                ExecutionQueryRequestItem::AddressDatastoreValueFinal {
                    addr,
                    key: key_a.clone(),
                },
                ExecutionQueryRequestItem::OpExecutionStatusCandidate(operation.id),
                ExecutionQueryRequestItem::OpExecutionStatusFinal(operation.id),
                ExecutionQueryRequestItem::AddressRollsCandidate(addr),
                ExecutionQueryRequestItem::AddressRollsFinal(addr),
                ExecutionQueryRequestItem::AddressDeferredCreditsCandidate(addr),
                ExecutionQueryRequestItem::AddressDeferredCreditsFinal(addr),
                ExecutionQueryRequestItem::CycleInfos {
                    cycle: 0,
                    restrict_to_addresses: None,
                },
                ExecutionQueryRequestItem::Events(EventFilter::default()),
            ],
        });
    // Just checking that is works no asserts for now
    universe
        .module_controller
        .get_addresses_infos(&[addr], std::ops::Bound::Unbounded);
}

/// This test checks causes a history rewrite in slot sequencing and ensures that emitted events match
#[test]
fn events_from_switching_blockclique() {
    // setup the period duration
    let exec_cfg = ExecutionConfig::default();
    let finalized_waitpoint = WaitPoint::new();
    let finalized_waitpoint2 = WaitPoint::new();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    expect_finalize_deploy_and_call_blocks(
        Slot::new(1, 0),
        None,
        finalized_waitpoint2.get_trigger_handle(),
        &mut foreign_controllers.final_state,
    );
    expect_finalize_deploy_and_call_blocks(
        Slot::new(1, 1),
        None,
        finalized_waitpoint.get_trigger_handle(),
        &mut foreign_controllers.final_state,
    );
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        None,
        None,
        None,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);

    // create blockclique block at slot (1,1)
    universe.deploy_bytecode_block(
        &KeyPair::from_str(TEST_SK_2).unwrap(),
        Slot::new(1, 1),
        include_bytes!("./wasm/event_test.wasm"),
        //unused in this case
        include_bytes!("./wasm/event_test.wasm"),
    );

    universe.deploy_bytecode_block(
        &KeyPair::from_str(TEST_SK_1).unwrap(),
        Slot::new(1, 0),
        include_bytes!("./wasm/event_test.wasm"),
        //unused in this case
        include_bytes!("./wasm/event_test.wasm"),
    );
    finalized_waitpoint2.wait();
    finalized_waitpoint.wait();
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter::default());
    assert_eq!(events.len(), 2, "wrong event count");
    assert_eq!(events[0].context.slot, Slot::new(1, 0), "Wrong event slot");
    assert_eq!(events[1].context.slot, Slot::new(1, 1), "Wrong event slot");
}

#[test]
fn not_enough_instance_gas() {
    // setup the period duration
    let exec_cfg = ExecutionConfig::default();
    let finalized_waitpoint = WaitPoint::new();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    expect_finalize_deploy_and_call_blocks(
        Slot::new(1, 0),
        None,
        finalized_waitpoint.get_trigger_handle(),
        &mut foreign_controllers.final_state,
    );
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        None,
        None,
        None,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);

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
        *CHAINID,
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
    universe.send_and_finalize(&keypair, block);
    finalized_waitpoint.wait();
    // assert events
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter::default());
    assert!(events[0]
        .data
        .contains("is lower than the base instance creation gas"));
}

#[test]
fn sc_builtins() {
    // setup execution config
    let exec_cfg = ExecutionConfig::default();
    let finalized_waitpoint = WaitPoint::new();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    expect_finalize_deploy_and_call_blocks(
        Slot::new(1, 0),
        None,
        finalized_waitpoint.get_trigger_handle(),
        &mut foreign_controllers.final_state,
    );
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        None,
        None,
        None,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);
    universe.deploy_bytecode_block(
        &keypair,
        Slot::new(1, 0),
        include_bytes!("./wasm/use_builtins.wasm"),
        //unused
        include_bytes!("./wasm/use_builtins.wasm"),
    );
    finalized_waitpoint.wait();
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
    assert!(events[0]
        .data
        .contains("abort with date and rnd at use_builtins.ts:0 col: 0"));
}

#[test]
fn validate_address() {
    // setup the period duration
    let exec_cfg = ExecutionConfig::default();
    let finalized_waitpoint = WaitPoint::new();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    expect_finalize_deploy_and_call_blocks(
        Slot::new(1, 0),
        None,
        finalized_waitpoint.get_trigger_handle(),
        &mut foreign_controllers.final_state,
    );
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        None,
        None,
        None,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);
    universe.deploy_bytecode_block(
        &keypair,
        Slot::new(1, 0),
        include_bytes!("./wasm/validate_address.wasm"),
        //unused
        include_bytes!("./wasm/use_builtins.wasm"),
    );
    finalized_waitpoint.wait();
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
}

#[test]
fn test_rewards() {
    // setup the period duration
    let exec_cfg = ExecutionConfig::default();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    let finalized_waitpoint = WaitPoint::new();
    let finalized_waitpoint_trigger_handle = finalized_waitpoint.get_trigger_handle();
    let finalized_waitpoint_trigger_handle_2 = finalized_waitpoint.get_trigger_handle();
    let endorsement_producer = KeyPair::generate(0).unwrap();
    let endorsement_producer_address =
        Address::from_public_key(&endorsement_producer.get_public_key());
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
    let keypair_address = Address::from_public_key(&keypair.get_public_key());
    let keypair2 = KeyPair::from_str(TEST_SK_2).unwrap();
    let keypair2_address = Address::from_public_key(&keypair2.get_public_key());
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        None,
        None,
        None,
    );
    foreign_controllers
        .final_state
        .write()
        .expect_finalize()
        .times(1)
        .with(predicate::eq(Slot::new(1, 0)), predicate::always())
        .returning(move |_, changes| {
            let block_credit_part = exec_cfg
                .block_reward
                .checked_div_u64(3 * (1 + (ENDORSEMENT_COUNT as u64)))
                .expect("critical: block_credits checked_div factor is 0")
                .saturating_mul_u64(2);
            let first_block_reward = exec_cfg.block_reward.saturating_sub(block_credit_part);
            assert_eq!(
                changes
                    .ledger_changes
                    .get_balance_or_else(&keypair_address, || None),
                // Reward + 100 base from boilerplate
                Some(
                    first_block_reward.saturating_add(Amount::from_mantissa_scale(100, 0).unwrap())
                )
            );

            assert_eq!(
                changes
                    .ledger_changes
                    .get_balance_or_else(&endorsement_producer_address, || None),
                // Reward + 100 base from boilerplate
                Some(
                    block_credit_part.saturating_add(Amount::from_mantissa_scale(100, 0).unwrap())
                )
            );
            finalized_waitpoint_trigger_handle.trigger();
        });

    foreign_controllers
        .final_state
        .write()
        .expect_finalize()
        .times(1)
        .with(predicate::eq(Slot::new(1, 1)), predicate::always())
        .returning(move |_, changes| {
            let block_credit_part_parent_in_thread = exec_cfg
                .block_reward
                .checked_div_u64(3 * (1 + (ENDORSEMENT_COUNT as u64)))
                .expect("critical: block_credits checked_div factor is 0")
                .saturating_mul_u64(ENDORSEMENT_COUNT as u64);
            let block_credit_part_endorsement_producer = exec_cfg
                .block_reward
                .checked_div_u64(3 * (1 + (ENDORSEMENT_COUNT as u64)))
                .expect("critical: block_credits checked_div factor is 0")
                .saturating_mul_u64(ENDORSEMENT_COUNT as u64);
            let creator_block_reward = exec_cfg
                .block_reward
                .saturating_sub(block_credit_part_endorsement_producer)
                .saturating_sub(block_credit_part_parent_in_thread);
            assert_eq!(
                changes
                    .ledger_changes
                    .get_balance_or_else(&keypair2_address, || None),
                // Reward + 100 base from boilerplate
                Some(
                    creator_block_reward
                        .saturating_add(Amount::from_mantissa_scale(100, 0).unwrap())
                )
            );

            assert_eq!(
                changes
                    .ledger_changes
                    .get_balance_or_else(&endorsement_producer_address, || None),
                // Reward + 100 base from boilerplate
                Some(
                    block_credit_part_endorsement_producer
                        .saturating_add(Amount::from_mantissa_scale(100, 0).unwrap())
                )
            );

            assert_eq!(
                changes
                    .ledger_changes
                    .get_balance_or_else(&keypair_address, || None),
                // Reward + 100 base from boilerplate
                Some(
                    block_credit_part_parent_in_thread
                        .saturating_add(Amount::from_mantissa_scale(100, 0).unwrap())
                )
            );
            finalized_waitpoint_trigger_handle_2.trigger();
        });
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());

    // First block
    let mut endorsements = vec![];
    for i in 0..ENDORSEMENT_COUNT {
        if i == 0 || i == 1 {
            endorsements.push(ExecutionTestUniverse::create_endorsement(
                &endorsement_producer,
                Slot::new(1, 0),
            ));
        } else {
            endorsements.push(ExecutionTestUniverse::create_endorsement(
                &keypair,
                Slot::new(1, 0),
            ));
        }
    }
    let block = ExecutionTestUniverse::create_block(
        &keypair,
        Slot::new(1, 0),
        vec![],
        endorsements,
        vec![],
    );
    universe.send_and_finalize(&keypair, block);
    finalized_waitpoint.wait();

    // Second block
    let block = ExecutionTestUniverse::create_block(
        &keypair2,
        Slot::new(1, 1),
        vec![],
        vec![
            ExecutionTestUniverse::create_endorsement(&endorsement_producer, Slot::new(1, 1));
            ENDORSEMENT_COUNT as usize
        ],
        vec![],
    );
    universe.send_and_finalize(&keypair, block);
    finalized_waitpoint.wait();
}

#[test]
fn chain_id() {
    // setup the period duration
    let exec_cfg = ExecutionConfig::default();
    let finalized_waitpoint = WaitPoint::new();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    expect_finalize_deploy_and_call_blocks(
        Slot::new(1, 0),
        None,
        finalized_waitpoint.get_trigger_handle(),
        &mut foreign_controllers.final_state,
    );
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        None,
        None,
        None,
    );
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);
    universe.deploy_bytecode_block(
        &keypair,
        Slot::new(1, 0),
        include_bytes!("./wasm/chain_id.wasm"),
        //unused
        include_bytes!("./wasm/chain_id.wasm"),
    );
    finalized_waitpoint.wait();
    let events = universe
        .module_controller
        .get_filtered_sc_output_event(EventFilter::default());
    // match the events
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, format!("Chain id: {}", *CHAINID));
}

#[cfg(feature = "execution-trace")]
#[test]
fn execution_trace() {
    // setup the period duration
    let mut exec_cfg = ExecutionConfig::default();
    // Make sure broadcast is enabled as we need it for this test
    exec_cfg.broadcast_enabled = true;

    let finalized_waitpoint = WaitPoint::new();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    expect_finalize_deploy_and_call_blocks(
        Slot::new(1, 0),
        None,
        finalized_waitpoint.get_trigger_handle(),
        &mut foreign_controllers.final_state,
    );
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        None,
        None,
        None,
    );

    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);
    universe.deploy_bytecode_block(
        &keypair,
        Slot::new(1, 0),
        include_bytes!("./wasm/execution_trace.wasm"),
        //unused
        include_bytes!("./wasm/execution_trace.wasm"),
    );
    finalized_waitpoint.wait();

    let mut receiver = universe.broadcast_traces_channel_receiver.take().unwrap();
    let join_handle = thread::spawn(move || loop {
        if let Ok(exec_traces) = receiver.blocking_recv() {
            if exec_traces.1 == true {
                return Ok::<
                    (massa_execution_exports::SlotAbiCallStack, bool),
                    tokio::sync::broadcast::error::RecvError,
                >(exec_traces);
            }
        }
    });
    let broadcast_result_ = join_handle.join().expect("Nothing received from thread");
    let (broadcast_result, _) = broadcast_result_.unwrap();

    let abi_name_1 = "assembly_script_generate_event";
    let traces_1: Vec<(OperationId, Vec<AbiTrace>)> = broadcast_result
        .operation_call_stacks
        .iter()
        .filter_map(|(k, v)| {
            Some((
                k.clone(),
                v.iter()
                    .filter(|t| t.name == abi_name_1)
                    .cloned()
                    .collect::<Vec<AbiTrace>>(),
            ))
        })
        .collect();

    assert_eq!(traces_1.len(), 1); // Only one op
    assert_eq!(traces_1.first().unwrap().1.len(), 1); // Only one generate_event
    assert_eq!(traces_1.first().unwrap().1.get(0).unwrap().name, abi_name_1);

    let abi_name_2 = "assembly_script_transfer_coins";
    let traces_2: Vec<(OperationId, Vec<AbiTrace>)> = broadcast_result
        .operation_call_stacks
        .iter()
        .filter_map(|(k, v)| {
            Some((
                k.clone(),
                v.iter()
                    .filter(|t| t.name == abi_name_2)
                    .cloned()
                    .collect::<Vec<AbiTrace>>(),
            ))
        })
        .collect();

    assert_eq!(traces_2.len(), 1); // Only one op
    assert_eq!(traces_2.first().unwrap().1.len(), 1); // Only one transfer_coins
    assert_eq!(traces_2.first().unwrap().1.get(0).unwrap().name, abi_name_2);
    // println!(
    //     "params: {:?}",
    //     traces_2.first().unwrap().1.get(0).unwrap().parameters
    // );
    assert_eq!(
        traces_2.first().unwrap().1.get(0).unwrap().parameters,
        vec![
            SCRuntimeAbiTraceValue {
                name: "from_address".to_string(),
                value: SCRuntimeAbiTraceType::String(
                    "AU1TyzwHarZMQSVJgxku8co7xjrRLnH74nFbNpoqNd98YhJkWgi".to_string()
                ),
            },
            SCRuntimeAbiTraceValue {
                name: "to_address".to_string(),
                value: SCRuntimeAbiTraceType::String(
                    "AU12E6N5BFAdC2wyiBV6VJjqkWhpz1kLVp2XpbRdSnL1mKjCWT6oR".to_string()
                ),
            },
            SCRuntimeAbiTraceValue {
                name: "raw_amount".to_string(),
                value: SCRuntimeAbiTraceType::U64(2000)
            }
        ]
    );
}

#[cfg(feature = "execution-trace")]
#[test]
fn execution_trace_nested() {
    // setup the period duration
    let mut exec_cfg = ExecutionConfig::default();
    // Make sure broadcast is enabled as we need it for this test
    exec_cfg.broadcast_enabled = true;

    let finalized_waitpoint = WaitPoint::new();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    let keypair = KeyPair::from_str(TEST_SK_1).unwrap();
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    expect_finalize_deploy_and_call_blocks(
        Slot::new(1, 0),
        None,
        finalized_waitpoint.get_trigger_handle(),
        &mut foreign_controllers.final_state,
    );
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        None,
        None,
        None,
    );

    // let rt = tokio::runtime::Runtime::new().unwrap();

    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg);
    universe.deploy_bytecode_block(
        &keypair,
        Slot::new(1, 0),
        include_bytes!("./wasm/et_deploy_sc.wasm"),
        include_bytes!("./wasm/et_init_sc.wasm"),
    );
    finalized_waitpoint.wait();

    let mut receiver = universe.broadcast_traces_channel_receiver.take().unwrap();
    let join_handle = thread::spawn(move || {
        // Execution Output
        loop {
            if let Ok(exec_traces) = receiver.blocking_recv() {
                if exec_traces.1 == true {
                    return Ok::<
                        (massa_execution_exports::SlotAbiCallStack, bool),
                        tokio::sync::broadcast::error::RecvError,
                    >(exec_traces);
                }
            }
        }
    });
    let broadcast_result_ = join_handle.join().expect("Nothing received from thread");

    // println!("b r: {:?}", broadcast_result_);
    let (broadcast_result, _) = broadcast_result_.unwrap();

    let abi_name_1 = "assembly_script_call";
    let traces_1: Vec<(OperationId, Vec<AbiTrace>)> = broadcast_result
        .operation_call_stacks
        .iter()
        .filter_map(|(k, v)| {
            Some((
                k.clone(),
                v.iter()
                    .filter(|t| t.name == abi_name_1)
                    .cloned()
                    .collect::<Vec<AbiTrace>>(),
            ))
        })
        .collect();

    assert_eq!(traces_1.len(), 1); // Only one op
    assert_eq!(traces_1.first().unwrap().1.len(), 1); // Only one transfer_coins
    assert_eq!(traces_1.first().unwrap().1.get(0).unwrap().name, abi_name_1);

    // filter sub calls
    let abi_name_2 = "assembly_script_transfer_coins";
    let sub_call: Vec<AbiTrace> = traces_1
        .first()
        .unwrap()
        .1
        .get(0)
        .unwrap()
        .sub_calls
        .as_ref()
        .unwrap()
        .iter()
        .filter(|a| a.name == abi_name_2)
        .cloned()
        .collect();

    // println!("params: {:?}", sub_call.get(0).unwrap().parameters);
    assert_eq!(
        sub_call.get(0).unwrap().parameters,
        vec![
            SCRuntimeAbiTraceValue {
                name: "from_address".to_string(),
                value: SCRuntimeAbiTraceType::String(
                    "AS1aEhosr1ebJJZ7cEMpSVKbY6xp1p4DdXabGb8fdkKKJ6WphGnR".to_string()
                )
            },
            SCRuntimeAbiTraceValue {
                name: "to_address".to_string(),
                value: SCRuntimeAbiTraceType::String(
                    "AU12E6N5BFAdC2wyiBV6VJjqkWhpz1kLVp2XpbRdSnL1mKjCWT6oR".to_string()
                )
            },
            SCRuntimeAbiTraceValue {
                name: "raw_amount".to_string(),
                value: SCRuntimeAbiTraceType::U64(1425)
            }
        ]
    );
}

#[cfg(feature = "dump-block")]
#[test]
fn test_dump_block() {
    use crate::storage_backend::StorageBackend;

    // setup the period duration
    let exec_cfg = ExecutionConfig::default();
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    let finalized_waitpoint = WaitPoint::new();
    let finalized_waitpoint_trigger_handle = finalized_waitpoint.get_trigger_handle();
    let recipient_address =
        Address::from_public_key(&KeyPair::generate(0).unwrap().get_public_key());
    selector_boilerplate(&mut foreign_controllers.selector_controller);
    final_state_boilerplate(
        &mut foreign_controllers.final_state,
        foreign_controllers.db.clone(),
        &foreign_controllers.selector_controller,
        &mut foreign_controllers.ledger_controller,
        None,
        None,
        None,
    );
    foreign_controllers
        .final_state
        .write()
        .expect_finalize()
        .times(1)
        .with(predicate::eq(Slot::new(1, 0)), predicate::always())
        .returning(move |_, changes| {
            // 190 because 100 in the get_balance in the `final_state_boilerplate` and 90 from the transfer.
            assert_eq!(
                changes
                    .ledger_changes
                    .get_balance_or_else(&recipient_address, || None),
                Some(Amount::from_str("190").unwrap())
            );
            // 1.02 for the block rewards
            assert_eq!(
                changes.ledger_changes.get_balance_or_else(
                    &Address::from_public_key(
                        &KeyPair::from_str(TEST_SK_1).unwrap().get_public_key()
                    ),
                    || None
                ),
                Some(
                    exec_cfg
                        .block_reward
                        .saturating_add(Amount::from_str("10").unwrap()) // add 10 fee
                )
            );
            finalized_waitpoint_trigger_handle.trigger();
        });
    let mut universe = ExecutionTestUniverse::new(foreign_controllers, exec_cfg.clone());
    // create the operation
    let operation = Operation::new_verifiable(
        Operation {
            fee: Amount::from_str("10").unwrap(),
            expire_period: 10,
            op: OperationType::Transaction {
                recipient_address,
                amount: Amount::from_str("90").unwrap(),
            },
        },
        OperationSerializer::new(),
        &KeyPair::from_str(TEST_SK_1).unwrap(),
        *CHAINID,
    )
    .unwrap();
    // create the block containing the transaction operation
    universe.storage.store_operations(vec![operation.clone()]);
    let block_slot = Slot::new(1, 0);
    let block = ExecutionTestUniverse::create_block(
        &KeyPair::from_str(TEST_SK_1).unwrap(),
        block_slot.clone(),
        vec![operation],
        vec![],
        vec![],
    );
    // store the block in storage
    universe.send_and_finalize(&KeyPair::from_str(TEST_SK_1).unwrap(), block);
    finalized_waitpoint.wait();

    std::thread::sleep(Duration::from_secs(1));

    // if the the storage backend for the dump-block feature is a rocksdb, this
    // is mandatory (the db must be closed before we can reopen it to ckeck the
    // data)
    drop(universe);

    let block_folder = &exec_cfg.block_dump_folder_path;
    #[cfg(all(feature = "file_storage_backend", not(feature = "db_storage_backend")))]
    let storage_backend =
        crate::storage_backend::FileStorageBackend::new(block_folder.to_owned(), 10);

    #[cfg(feature = "db_storage_backend")]
    let storage_backend =
        crate::storage_backend::RocksDBStorageBackend::new(block_folder.to_owned(), 10);

    let block_content = storage_backend.read(&block_slot).unwrap();
    let filled_block = FilledBlock::decode(&mut Cursor::new(block_content)).unwrap();
    let header_content = filled_block.header.unwrap().content.unwrap();
    let header_slot = header_content.slot.unwrap();
    assert_eq!(header_slot.thread, u32::from(block_slot.thread));
    assert_eq!(header_slot.period, block_slot.period);
    assert_eq!(header_content.endorsements.len(), 0);
    assert_eq!(filled_block.operations.len(), 1);
}
