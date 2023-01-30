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
use std::time::Duration;

use crate::tests::tools::create_some_operations;
use crate::tests::tools::pool_test;
use massa_execution_exports::test_exports::MockExecutionControllerMessage as ControllerMsg;
use massa_models::address::Address;
use massa_models::operation::OperationId;
use massa_models::prehash::PreHashSet;
use massa_models::slot::Slot;
use massa_pool_exports::PoolConfig;
use massa_signature::KeyPair;

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
#[ignore]
fn test_simple_get_operations() {
    let config = PoolConfig::default();
    pool_test(
        config,
        |mut pool_manager, mut pool_controller, execution_receiver, mut storage| {
            let keypair = KeyPair::generate(1).unwrap();
            storage.store_operations(create_some_operations(10, &keypair, 1));

            let creator_address = Address::from_public_key(&keypair.get_public_key());
            let creator_thread = creator_address.get_thread(config.thread_count);
            let unexecuted_ops = storage.get_op_refs().clone();
            pool_controller.add_operations(storage);

            // start mock execution thread
            std::thread::spawn(move || {
                // TODO following behavior is not valid anymore
                match execution_receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(ControllerMsg::UnexecutedOpsAmong { response_tx, .. }) => {
                        response_tx.send(unexecuted_ops.clone()).unwrap();
                    }
                    Ok(_) => panic!("unexpected controller request"),
                    Err(_) => panic!("execution never called"),
                }
                match execution_receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(ControllerMsg::GetFinalAndCandidateBalance {
                        addresses,
                        response_tx,
                        ..
                    }) => {
                        assert_eq!(addresses.len(), 1);
                        assert_eq!(addresses[0], creator_address);
                        response_tx.send(vec![]).unwrap();
                    }
                    Ok(_) => panic!("unexpected controller request"),
                    Err(_) => panic!("execution never called"),
                }
                (0..9).for_each(|_| {
                    match execution_receiver.recv_timeout(Duration::from_millis(100)) {
                        Ok(ControllerMsg::UnexecutedOpsAmong { response_tx, .. }) => {
                            response_tx.send(unexecuted_ops.clone()).unwrap();
                        }
                        Ok(_) => panic!("unexpected controller request"),
                        Err(_) => panic!("execution never called"),
                    }
                })
            });

            let block_operations_storage = pool_controller
                .get_block_operations(&Slot::new(1, creator_thread))
                .1;

            pool_manager.stop();

            assert_eq!(block_operations_storage.get_op_refs().len(), 10);
        },
    );
}

/// Launch a default mock for execution controller on call `get_block_operation` API.
fn launch_basic_get_block_operation_execution_mock(
    operations_len: usize,
    unexecuted_ops: PreHashSet<OperationId>,
    recvr: Receiver<ControllerMsg>,
) {
    let receive = |er: &Receiver<ControllerMsg>| er.recv_timeout(Duration::from_millis(10));
    std::thread::spawn(move || {
        use ControllerMsg::GetFinalAndCandidateBalance as GetFinal;
        use ControllerMsg::UnexecutedOpsAmong as Unexecuted;

        if let Ok(Unexecuted { response_tx, .. }) = receive(&recvr) {
            response_tx.send(unexecuted_ops.clone()).unwrap();
        }
        if let Ok(GetFinal { response_tx, .. }) = receive(&recvr) {
            response_tx.send(vec![]).unwrap();
        }
        (1..operations_len).for_each(|_| {
            if let Ok(Unexecuted { response_tx, .. }) = receive(&recvr) {
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
#[ignore]
fn test_get_operations_overflow() {
    static OP_LEN: usize = 10;
    static MAX_OP_LEN: usize = 5;
    let mut max_block_size = 0;
    let keypair = KeyPair::generate(1).unwrap();
    let creator_address = Address::from_public_key(&keypair.get_public_key());
    let operations = create_some_operations(OP_LEN, &keypair, 1);
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
        |mut pool_manager, mut pool_controller, execution_receiver, mut storage| {
            storage.store_operations(operations);

            let unexecuted_ops = storage.get_op_refs().clone();
            pool_controller.add_operations(storage);

            // start mock execution thread
            launch_basic_get_block_operation_execution_mock(
                OP_LEN,
                unexecuted_ops,
                execution_receiver,
            );

            let block_operations_storage = pool_controller
                .get_block_operations(&Slot::new(1, creator_thread))
                .1;

            pool_manager.stop();

            assert_eq!(block_operations_storage.get_op_refs().len(), MAX_OP_LEN);
        },
    );
}
