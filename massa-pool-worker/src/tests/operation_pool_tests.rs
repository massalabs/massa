// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! # Internal Pool units tests
//! Units tests that are internals to pool and do not require any foreign
//! modules to be tested.
//!
//! # Add operations
//! Function: [`test_add_operation`]
//! Classic usage of internal `add_operations` function from the [`OperationPool`].
//!
//! # Add irrelevant operation
//! Function: [`test_add_irrelevant_operation`]
//! Same as classic but we try to add irrelevant operation. (See the definition
//! chapter below)
//!
//! # Definition
//! Relevant operation: Operation with a validity range corresponding to the
//! latest period given his own thread. All operation which doesn't fit these
//! requirements are "irrelevant"
//!
use crate::operation_pool::OperationPool;
use crate::tests::tools::OpGenerator;

use super::tools::{
    create_some_operations, default_mock_execution_controller, pool_test, PoolTestBoilerPlate,
};
use massa_execution_exports::MockExecutionController;
use massa_models::{
    address::Address, amount::Amount, config::ENDORSEMENT_COUNT, operation::OperationId,
    prehash::PreHashMap, slot::Slot,
};
use massa_pool_exports::{PoolBroadcasts, PoolChannels, PoolConfig};
use massa_pos_exports::{MockSelectorController, Selection};
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_wallet::test_exports::create_test_wallet;
use parking_lot::RwLock;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::broadcast;

// Helper to create a recursive selector mock for operation pool tests
fn create_recursive_selector_for_ops(addr: Address) -> MockSelectorController {
    let mut story = MockSelectorController::new();
    story
        .expect_clone_box()
        .returning(move || Box::new(create_recursive_selector_for_ops(addr)));
    story
        .expect_get_available_selections_in_range()
        .returning(move |slot_range, opt_addrs| {
            let mut all_slots = BTreeMap::new();
            let address = *opt_addrs
                .expect("No addresses filter given")
                .iter()
                .next()
                .expect("No addresses given");
            for i in 0..15 {
                for j in 0..32 {
                    let s = Slot::new(i, j);
                    if slot_range.contains(&s) {
                        all_slots.insert(
                            s,
                            Selection {
                                producer: address,
                                endorsements: vec![addr; ENDORSEMENT_COUNT as usize],
                            },
                        );
                    }
                }
            }
            Ok(all_slots)
        });
    story
}

/// Speculative/candidate-only execution must not permanently remove ops from the pool.
/// They must also be skipped for block production while the mark is live (no gas waste),
/// and become selectable again after a rollback clears the mark.
#[test]
fn test_refresh_keeps_speculative_only_executed_ops() {
    let keypair = KeyPair::generate(0).unwrap();
    let addr = Address::from_public_key(&keypair.get_public_key());
    let creator_thread = addr.get_thread(PoolConfig::default().thread_count);

    // 0 = not executed, 1 = speculative only, 2 = final
    let exec_phase = Arc::new(AtomicU8::new(0));
    let phase_for_status = exec_phase.clone();
    let mut execution_controller = MockExecutionController::new();
    execution_controller
        .expect_get_ops_exec_status()
        .returning(move |ops| match phase_for_status.load(Ordering::SeqCst) {
            1 => vec![(Some(true), None); ops.len()],
            2 => vec![(Some(true), Some(true)); ops.len()],
            _ => vec![(None, None); ops.len()],
        });
    execution_controller
        .expect_get_final_and_candidate_balance()
        .returning(|addrs| {
            vec![
                (
                    Some(Amount::const_init(1_000_000_000, 0)),
                    Some(Amount::const_init(1_000_000_000, 0)),
                );
                addrs.len()
            ]
        });

    let mut addresses = PreHashMap::default();
    addresses.insert(addr, keypair.clone());
    let wallet = Arc::new(RwLock::new(create_test_wallet(Some(addresses))));
    let (endorsement_sender, _) = broadcast::channel(1);
    let (operation_sender, _) = broadcast::channel(1);

    let storage = Storage::create_root();
    let mut operation_pool = OperationPool::init(
        PoolConfig::default(),
        &storage,
        PoolChannels {
            execution_controller: Box::new(execution_controller),
            broadcasts: PoolBroadcasts {
                endorsement_sender,
                operation_sender,
            },
            selector: Box::new(create_recursive_selector_for_ops(addr)),
        },
        wallet,
    );

    let ops = create_some_operations(
        3,
        &OpGenerator::default()
            .creator(keypair)
            .expirery(10)
            .fee(Amount::from_raw(1)),
    );
    let op_ids: Vec<OperationId> = ops.iter().map(|op| op.id).collect();
    let mut ops_storage = storage.clone_without_refs();
    ops_storage.store_operations(ops);
    operation_pool.add_operations(ops_storage);
    assert_eq!(operation_pool.len(), 3);

    let target_slot = Slot::new(1, creator_thread);

    // Candidate-history mark only: ops must remain after refresh, but not be block-selected.
    exec_phase.store(1, Ordering::SeqCst);
    operation_pool.refresh();
    assert_eq!(operation_pool.len(), 3);
    for id in &op_ids {
        assert!(operation_pool.contains(id));
    }
    let (selected, _) = operation_pool.get_block_operations(&target_slot);
    assert!(
        selected.is_empty(),
        "speculatively executed ops must not be selected for blocks"
    );

    // Simulate rollback clearing the speculative mark: still present and selectable again.
    exec_phase.store(0, Ordering::SeqCst);
    operation_pool.refresh();
    assert_eq!(operation_pool.len(), 3);
    let (selected, _) = operation_pool.get_block_operations(&target_slot);
    assert_eq!(selected.len(), 3);

    // Final execution: durable, so refresh may drop them.
    exec_phase.store(2, Ordering::SeqCst);
    operation_pool.refresh();
    assert_eq!(operation_pool.len(), 0);
}

#[test]
fn test_add_operation() {
    use massa_signature::KeyPair;
    let execution_controller = default_mock_execution_controller();
    let addr = Address::from_public_key(&KeyPair::generate(0).unwrap().get_public_key());
    let selector_controller = Box::new(create_recursive_selector_for_ops(addr));
    pool_test(
        PoolConfig::default(),
        execution_controller,
        selector_controller,
        None,
        |mut operation_pool, mut storage| {
            let op_gen = OpGenerator::default().expirery(2);
            storage.store_operations(create_some_operations(10, &op_gen));
            operation_pool.add_operations(storage).unwrap();
            // Allow some time for the pool to add the operations
            std::thread::sleep(Duration::from_secs(3));
            assert_eq!(
                operation_pool
                    .get_operation_count(None)
                    .expect("Failed to get operation count"),
                10
            );
        },
    );
}

/// Test if adding irrelevant operations make simply skip the add.
/// # Initialization
#[test]
fn test_add_irrelevant_operation() {
    use massa_signature::KeyPair;
    let pool_config = PoolConfig::default();
    let thread_count = pool_config.thread_count;
    let execution_controller = default_mock_execution_controller();
    let addr = Address::from_public_key(&KeyPair::generate(0).unwrap().get_public_key());
    let selector_controller = Box::new(create_recursive_selector_for_ops(addr));
    pool_test(
        PoolConfig::default(),
        execution_controller,
        selector_controller,
        None,
        |mut operation_pool, mut storage| {
            let op_gen = OpGenerator::default().expirery(2);
            storage.store_operations(create_some_operations(10, &op_gen));
            operation_pool.notify_final_cs_periods(&vec![51; thread_count.into()]);
            operation_pool.add_operations(storage).unwrap();
            // Allow some time for the pool to add the operations
            std::thread::sleep(Duration::from_secs(3));
            assert_eq!(
                operation_pool
                    .get_operation_count(None)
                    .expect("Failed to get operation count"),
                0
            );
        },
    );
}

#[test]
fn test_pool() {
    use massa_signature::KeyPair;
    let pool_config = PoolConfig {
        max_operations_per_block: 10,
        ..Default::default()
    };
    let execution_controller = default_mock_execution_controller();
    let addr = Address::from_public_key(&KeyPair::generate(0).unwrap().get_public_key());
    let selector_controller = Box::new(create_recursive_selector_for_ops(addr));
    let PoolTestBoilerPlate {
        mut pool_manager,
        mut pool_controller,
        storage: storage_base,
    } = PoolTestBoilerPlate::pool_test(pool_config, execution_controller, selector_controller);

    // // generate (id, transactions, range of validity) by threads
    let mut thread_tx_lists = vec![Vec::new(); pool_config.thread_count as usize];

    let mut storage = storage_base.clone_without_refs();
    for i in 0..500 {
        let expire_period = 3;
        let op = OpGenerator::default()
            .expirery(expire_period)
            .fee(Amount::const_init(1 + i, 3)) // can panic but not a big deal as we are testing
            .generate(); //get_transaction(expire_period, fee);

        storage.store_operations(vec![op.clone()]);
        let op_thread = op
            .content_creator_address
            .get_thread(pool_config.thread_count);

        let start_period = expire_period.saturating_sub(pool_config.operation_validity_periods);

        thread_tx_lists[op_thread as usize].push((op, start_period..=expire_period));
    }

    pool_controller.add_operations(storage).unwrap();
    std::thread::sleep(Duration::from_secs(3));
    // // sort from bigger fee to smaller and truncate
    for lst in thread_tx_lists.iter_mut() {
        lst.reverse();
        lst.truncate(pool_config.max_operations_per_block as usize);
    }

    // // checks ops are the expected ones for thread 0 and 1 and various periods
    for thread in 0u8..pool_config.thread_count {
        let target_slot = Slot::new(0, thread);
        let (ids, storage) = pool_controller
            .get_block_operations(&target_slot, None)
            .expect("Failed to get block operations");

        assert_eq!(
            ids.iter()
                .map(|id| (
                    *id,
                    storage
                        .read_operations()
                        .get(id)
                        .unwrap()
                        .serialized_data
                        .clone()
                ))
                .collect::<Vec<(OperationId, Vec<u8>)>>(),
            thread_tx_lists[target_slot.thread as usize]
                .iter()
                .filter(|(_, r)| r.contains(&target_slot.period))
                .map(|(op, _)| (op.id, op.serialized_data.clone()))
                .collect::<Vec<(OperationId, Vec<u8>)>>()
        );
    }
    pool_manager.stop();
}
