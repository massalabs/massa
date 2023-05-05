// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! # External Pool units tests
//! Units tests scenarios that use the Pool controller API and check IO given
//! configurations and foreign modules initialization.
//!
//! # Get operations
//! Function: [`test_simple_get_operations`]
//! Scenario adding operations to pool then get the operations for a slot
//!
//! //! # Get operations overflow
//! Function: [`test_get_operations_overflow`]
//! Same as the previous test with a low limit of size to check if
//! configurations are taken into account.

use std::sync::mpsc::Receiver;
use std::thread;
use std::time::Duration;

use crate::start_pool_controller;
use crate::tests::tools::create_some_operations;
use crate::tests::tools::pool_test;
use crate::tests::tools::OpGenerator;
use massa_execution_exports::test_exports;
use massa_execution_exports::MockExecutionController;
use massa_models::address::Address;
use massa_models::amount::Amount;
use massa_models::denunciation::{Denunciation, DenunciationIndex, DenunciationPrecursor};
use massa_models::operation::OperationId;
use massa_models::prehash::PreHashSet;
use massa_models::slot::Slot;
use massa_models::test_exports::{
    gen_block_headers_for_denunciation, gen_endorsements_for_denunciation,
};
use massa_pool_exports::PoolChannels;
use massa_pool_exports::PoolConfig;
use massa_pos_exports::test_exports::MockSelectorController;
use massa_pos_exports::test_exports::MockSelectorControllerMessage;
use massa_pos_exports::{PosResult, Selection};
use massa_signature::KeyPair;
use massa_storage::Storage;
use tokio::sync::broadcast;

/// # Test simple get operation
/// Just try to get some operations stored in pool
///
/// ## Initialization
/// Insert multiple operations in the pool. (10)
///
/// Start mocked execution controller thread. (expected 2 calls of `unexecuted_ops_among`
/// that return the full storage)
/// The execution thread will response that no operations had been executed.
///
/// ## Expected results
/// The execution controller is expected to be asked 2 times for the first interaction:
/// - to check the already executed operations
/// - to check the final and candidate balances of the creator address
/// And one time for the 9 next to check the executed operations.
///
/// The block operation storage built for all threads is expected to have the
/// same length than those added previously.
#[test]
fn test_simple_get_operations() {
    let config = PoolConfig::default();
    let mut storage: Storage = Storage::create_root();
    let endorsement_sender = broadcast::channel(2000).0;
    let operation_sender = broadcast::channel(5000).0;
    let (execution_controller, execution_receiver) =
        test_exports::MockExecutionController::new_with_receiver();
    let (selector_controller, selector_receiver) = MockSelectorController::new_with_receiver();

    let (mut pool_manager, mut pool_controller) = start_pool_controller(
        config,
        &storage,
        execution_controller,
        PoolChannels {
            endorsement_sender,
            operation_sender,
            selector: selector_controller,
        },
    );

    //setup meta-data
    let keypair = KeyPair::generate();
    let op_gen = OpGenerator::default().creator(keypair.clone()).expirery(1);
    let creator_address = Address::from_public_key(&keypair.get_public_key());
    let creator_thread = creator_address.get_thread(config.thread_count);

    // setup storage
    storage.store_operations(create_some_operations(10, &op_gen));
    let unexecuted_ops = storage.get_op_refs().clone();
    pool_controller.add_operations(storage);

    // Allow some time for the pool to add the operations
    std::thread::sleep(Duration::from_millis(100));

    // Start mock execution thread.
    // Provides the data for `pool_controller.get_block_operations`
    {
        let creator_address = creator_address;
        let balance_vec = vec![(Some(Amount::from_raw(1)), Some(Amount::from_raw(1)))];
        let receive = |er: &Receiver<test_exports::MockExecutionControllerMessage>| {
            er.recv_timeout(Duration::from_millis(100))
        };
        std::thread::spawn(move || {
            match receive(&execution_receiver) {
                Ok(test_exports::MockExecutionControllerMessage::UnexecutedOpsAmong {
                    response_tx,
                    ..
                }) => response_tx.send(unexecuted_ops.clone()).unwrap(),
                Ok(op) => panic!("Expected `ControllerMsg::UnexecutedOpsAmong`, got {:?}", op),
                Err(_) => panic!("execution never called"),
            }
            match receive(&execution_receiver) {
                Ok(test_exports::MockExecutionControllerMessage::GetFinalAndCandidateBalance {
                    addresses,
                    response_tx,
                    ..
                }) => {
                    assert_eq!(addresses.len(), 1);
                    assert_eq!(addresses[0], creator_address);
                    response_tx.send(balance_vec).unwrap();
                }
                Ok(op) => panic!(
                    "Expected `ControllerMsg::GetFinalAndCandidateBalance`, got {:?}",
                    op
                ),
                Err(_) => panic!("execution never called"),
            }

            (1..10).for_each(|_| {
                if let Ok(test_exports::MockExecutionControllerMessage::UnexecutedOpsAmong {
                    response_tx,
                    ..
                }) = receive(&execution_receiver)
                {
                    response_tx.send(unexecuted_ops.clone()).unwrap();
                }
            })
        });
    };

    // This is what we are testing....
    let block_operations_storage = pool_controller
        .get_block_operations(&Slot::new(1, creator_thread))
        .1;

    pool_manager.stop();

    assert_eq!(block_operations_storage.get_op_refs().len(), 10);
}

/// Launch a default mock for execution controller on call `get_block_operation` API.
pub fn launch_basic_get_block_operation_execution_mock(
    operations_len: usize,
    unexecuted_ops: PreHashSet<OperationId>,
    recvr: Receiver<test_exports::MockExecutionControllerMessage>,
    creator_address: Address,
    balance_vec: Vec<(Option<Amount>, Option<Amount>)>,
) {
    let receive = |er: &Receiver<test_exports::MockExecutionControllerMessage>| {
        er.recv_timeout(Duration::from_millis(100))
    };
    std::thread::spawn(move || {
        match receive(&recvr) {
            Ok(test_exports::MockExecutionControllerMessage::UnexecutedOpsAmong {
                response_tx,
                ..
            }) => response_tx.send(unexecuted_ops.clone()).unwrap(),
            Ok(op) => panic!("Expected `ControllerMsg::UnexecutedOpsAmong`, got {:?}", op),
            Err(_) => panic!("execution never called"),
        }
        match receive(&recvr) {
            Ok(test_exports::MockExecutionControllerMessage::GetFinalAndCandidateBalance {
                addresses,
                response_tx,
                ..
            }) => {
                assert_eq!(addresses.len(), 1);
                assert_eq!(addresses[0], creator_address);
                response_tx.send(balance_vec).unwrap();
            }
            Ok(op) => panic!(
                "Expected `ControllerMsg::GetFinalAndCandidateBalance`, got {:?}",
                op
            ),
            Err(_) => panic!("execution never called"),
        }

        (1..operations_len).for_each(|_| {
            if let Ok(test_exports::MockExecutionControllerMessage::UnexecutedOpsAmong {
                response_tx,
                ..
            }) = receive(&recvr)
            {
                response_tx.send(unexecuted_ops.clone()).unwrap();
            }
        })
    });
}

/// # Test get block operation with overflow
/// Try to get some operations stored in pool for a block, but pool's operations
/// are bigger than the max block's size.
///
/// ## Initialization
/// Create 10 operations.
/// Compute size of 5 of these and set `max_block_size`.
/// Add 10 operations to pool.
///
/// Start mocked execution controller thread.
///
/// ## Expected result
/// The block operation storage built for all threads is expected to have
/// only 5 operations.
#[test]
fn test_get_operations_overflow() {
    // setup metadata
    static OP_LEN: usize = 10;
    static MAX_OP_LEN: usize = 5;
    let mut max_block_size = 0;
    let keypair = KeyPair::generate();
    let creator_address = Address::from_public_key(&keypair.get_public_key());
    let op_gen = OpGenerator::default().expirery(1).creator(keypair);
    let operations = create_some_operations(OP_LEN, &op_gen);
    operations
        .iter()
        .take(MAX_OP_LEN)
        .for_each(|op| max_block_size += op.serialized_size() as u32);
    let config = PoolConfig {
        max_block_size,
        ..Default::default()
    };
    let creator_thread = creator_address.get_thread(config.thread_count);
    pool_test(
        config,
        |mut pool_manager,
         mut pool_controller,
         execution_receiver,
         _selector_receiver,
         mut storage| {
            // setup storage
            storage.store_operations(operations);
            let unexecuted_ops = storage.get_op_refs().clone();
            pool_controller.add_operations(storage);
            // Allow some time for the pool to add the operations
            std::thread::sleep(Duration::from_millis(100));

            // start mock execution thread
            launch_basic_get_block_operation_execution_mock(
                OP_LEN,
                unexecuted_ops,
                execution_receiver,
                creator_address,
                vec![(Some(Amount::from_raw(1)), Some(Amount::from_raw(1)))],
            );

            let block_operations_storage = pool_controller
                .get_block_operations(&Slot::new(1, creator_thread))
                .1;

            pool_manager.stop();

            assert_eq!(block_operations_storage.get_op_refs().len(), MAX_OP_LEN);
        },
    );
}

#[test]
fn test_block_header_denunciation_creation() {
    let (_slot, keypair, secured_header_1, secured_header_2, _secured_header_3) =
        gen_block_headers_for_denunciation(None, None);
    let address = Address::from_public_key(&keypair.get_public_key());

    let de_p_1 = DenunciationPrecursor::from(&secured_header_1);
    let de_p_2 = DenunciationPrecursor::from(&secured_header_2);

    // Built it to compare with what the factory will produce
    let denunciation_orig = Denunciation::try_from((&secured_header_1, &secured_header_2)).unwrap();

    let config = PoolConfig::default();
    pool_test(
        config,
        |mut pool_manager, pool_controller, _execution_receiver, selector_receiver, _storage| {
            pool_controller.add_denunciation_precursor(de_p_1);
            pool_controller.add_denunciation_precursor(de_p_2);
            // Allow some time for the pool to add the operations
            loop {
                match selector_receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(MockSelectorControllerMessage::GetProducer {
                        slot: _slot,
                        response_tx,
                    }) => {
                        response_tx.send(PosResult::Ok(address)).unwrap();
                    }
                    Ok(msg) => {
                        panic!(
                            "Received an unexpected message from mock selector: {:?}",
                            msg
                        );
                    }
                    Err(_e) => {
                        // timeout
                        break;
                    }
                }
            }

            assert_eq!(pool_controller.get_denunciation_count(), 1);
            assert_eq!(
                pool_controller.contains_denunciation(&denunciation_orig),
                true
            );

            pool_manager.stop();
        },
    );
}

#[test]
fn test_endorsement_denunciation_creation() {
    let (_slot, keypair, s_endorsement_1, s_endorsement_2, _) =
        gen_endorsements_for_denunciation(None, None);
    let address = Address::from_public_key(&keypair.get_public_key());

    let de_p_1 = DenunciationPrecursor::from(&s_endorsement_1);
    let de_p_2 = DenunciationPrecursor::from(&s_endorsement_2);

    // Built it to compare with what the factory will produce
    let denunciation_orig = Denunciation::try_from((&s_endorsement_1, &s_endorsement_2)).unwrap();

    let config = PoolConfig::default();
    pool_test(
        config,
        |mut pool_manager, pool_controller, _execution_receiver, selector_receiver, _storage| {
            pool_controller.add_denunciation_precursor(de_p_1);
            pool_controller.add_denunciation_precursor(de_p_2);
            // Allow some time for the pool to add the operations
            loop {
                match selector_receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(MockSelectorControllerMessage::GetSelection {
                        slot: _slot,
                        response_tx,
                    }) => {
                        let selection = Selection {
                            endorsements: vec![address; usize::from(config.thread_count)],
                            producer: address,
                        };

                        response_tx.send(PosResult::Ok(selection)).unwrap();
                    }
                    Ok(msg) => {
                        panic!(
                            "Received an unexpected message from mock selector: {:?}",
                            msg
                        );
                    }
                    Err(_e) => {
                        // timeout
                        break;
                    }
                }
            }

            assert_eq!(pool_controller.get_denunciation_count(), 1);
            assert_eq!(
                pool_controller.contains_denunciation(&denunciation_orig),
                true
            );

            pool_manager.stop();
        },
    );
}

#[test]
fn test_denunciation_pool_get() {
    // gen a endorsement denunciation
    let (_slot, keypair_1, s_endorsement_1, s_endorsement_2, _) =
        gen_endorsements_for_denunciation(None, None);
    let address_1 = Address::from_public_key(&keypair_1.get_public_key());

    let de_p_1 = DenunciationPrecursor::from(&s_endorsement_1);
    let de_p_2 = DenunciationPrecursor::from(&s_endorsement_2);
    let denunciation_orig_1 = Denunciation::try_from((&s_endorsement_1, &s_endorsement_2)).unwrap();

    // gen a block header denunciation
    let (_slot, keypair_2, secured_header_1, secured_header_2, _secured_header_3) =
        gen_block_headers_for_denunciation(None, None);
    let address_2 = Address::from_public_key(&keypair_2.get_public_key());

    let de_p_3 = DenunciationPrecursor::from(&secured_header_1);
    let de_p_4 = DenunciationPrecursor::from(&secured_header_2);
    let denunciation_orig_2 =
        Denunciation::try_from((&secured_header_1, &secured_header_2)).unwrap();
    let de_idx_2 = DenunciationIndex::from(&denunciation_orig_2);

    let config = PoolConfig::default();
    pool_test(
        config,
        |mut pool_manager, pool_controller, execution_receiver, selector_receiver, _storage| {
            // ~ random order (but need to keep the precursor order otherwise Denunciation::PartialEq will fail)
            pool_controller.add_denunciation_precursor(de_p_3);
            pool_controller.add_denunciation_precursor(de_p_1);
            pool_controller.add_denunciation_precursor(de_p_4);
            pool_controller.add_denunciation_precursor(de_p_2);

            // Allow some time for the pool to add the operations
            loop {
                match selector_receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(MockSelectorControllerMessage::GetProducer {
                        slot: _slot,
                        response_tx,
                    }) => {
                        response_tx.send(PosResult::Ok(address_2)).unwrap();
                    }
                    Ok(MockSelectorControllerMessage::GetSelection {
                        slot: _slot,
                        response_tx,
                    }) => {
                        let selection = Selection {
                            endorsements: vec![address_1; usize::from(config.thread_count)],
                            producer: address_1,
                        };

                        response_tx.send(PosResult::Ok(selection)).unwrap();
                    }
                    Ok(msg) => {
                        panic!(
                            "Received an unexpected message from mock selector: {:?}",
                            msg
                        );
                    }
                    Err(_) => {
                        // timeout, exit the loop
                        break;
                    }
                }
            }

            assert_eq!(pool_controller.get_denunciation_count(), 2);
            assert_eq!(
                pool_controller.contains_denunciation(&denunciation_orig_1),
                true
            );
            assert_eq!(
                pool_controller.contains_denunciation(&denunciation_orig_2),
                true
            );

            // Now ask for denunciations
            // Note that we need 2 threads as the get_block_denunciations call will wait for
            // the mock execution controller to return

            let target_slot_1 = Slot::new(4, 0);
            let thread_1 = thread::spawn(move || loop {
                match execution_receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(test_exports::MockExecutionControllerMessage::IsDenunciationExecuted {
                        de_idx,
                        response_tx,
                    }) => {
                        // Note: this should prevent denunciation_orig_1 to be included
                        if de_idx == de_idx_2 {
                            response_tx.send(false).unwrap();
                        } else {
                            response_tx.send(true).unwrap();
                        }
                    }
                    Ok(msg) => {
                        panic!(
                            "Received an unexpected message from mock execution: {:?}",
                            msg
                        );
                    }
                    Err(_) => break,
                }
            });
            let thread_2 =
                thread::spawn(move || pool_controller.get_block_denunciations(&target_slot_1));

            thread_1.join().unwrap();
            let denunciations = thread_2.join().unwrap();

            assert_eq!(denunciations, vec![denunciation_orig_2]);

            pool_manager.stop();
        },
    )
}
