use std::{str::FromStr, sync::Arc};

use super::BlockTestFactory;
use massa_consensus_exports::MockConsensusController;
use massa_hash::Hash;
use massa_models::config::CHAINID;
use massa_models::{
    address::Address,
    amount::Amount,
    block_id::BlockId,
    config::THREAD_COUNT,
    operation::{Operation, OperationSerializer, OperationType},
    secure_share::SecureShareContent,
    slot::Slot,
};
use massa_pool_exports::MockPoolController;
use massa_pos_exports::MockSelectorController;
use massa_signature::KeyPair;
use massa_storage::Storage;
use parking_lot::{Condvar, Mutex};
use serial_test::serial;

/// Creates a basic empty block with the factory.
#[test]
#[serial]
fn basic_creation() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));
    let keypair = KeyPair::generate(0).unwrap();
    let storage = Storage::create_root();
    let staking_address = Address::from_public_key(&keypair.get_public_key());
    let parent = BlockId::generate_from_hash(Hash::compute_from("test".as_bytes()));
    let mut parents = Vec::new();
    for i in 0..THREAD_COUNT as u64 {
        parents.push((parent, i));
    }
    let pair = Arc::new((Mutex::new(false), Condvar::new()));
    let pair2 = pair.clone();
    let mut consensus_controller = Box::new(MockConsensusController::new());
    consensus_controller
        .expect_get_best_parents()
        .times(1)
        .return_once(move || parents);
    consensus_controller
        .expect_register_block()
        .times(1)
        .return_once(move |_, _, storage, created| {
            assert!(created);
            let block = storage.get_block_refs();
            assert_eq!(block.len(), 1);
            let (lock, cvar) = &*pair2;
            let mut started = lock.lock();
            *started = true;
            cvar.notify_one();
        });
    let mut selector_controller = Box::new(MockSelectorController::new());
    selector_controller
        .expect_get_producer()
        .times(1)
        .return_once(move |_| Ok(staking_address));
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller
        .expect_get_block_denunciations()
        .returning(|slot, _timeout| {
            assert_eq!(*slot, Slot::new(1, 0));
            Ok(vec![])
        });
    pool_controller
        .expect_get_block_operations()
        .returning(|slot, _timeout| {
            assert_eq!(*slot, Slot::new(1, 0));
            Ok((vec![], Storage::create_root()))
        });
    pool_controller
        .expect_get_block_endorsements()
        .returning(|_, slot, _timeout| {
            assert_eq!(*slot, Slot::new(1, 0));
            Ok((vec![], Storage::create_root()))
        });
    let mut test_factory = BlockTestFactory::new(
        &keypair,
        storage,
        consensus_controller,
        selector_controller,
        pool_controller,
    );
    let (ref lock, ref cvar) = *pair;
    let mut started = lock.lock();
    if !*started {
        cvar.wait(&mut started);
    }
    test_factory.stop();
}

/// Creates a block with a roll buy operation in it.
#[test]
#[serial]
fn basic_creation_with_operation() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));
    let keypair = KeyPair::generate(0).unwrap();
    let staking_address = Address::from_public_key(&keypair.get_public_key());
    let parent = BlockId::generate_from_hash(Hash::compute_from("test".as_bytes()));
    let mut parents = Vec::new();
    for i in 0..THREAD_COUNT as u64 {
        parents.push((parent, i));
    }
    let mut selector_controller = Box::new(MockSelectorController::new());
    selector_controller
        .expect_get_producer()
        .times(1)
        .return_once(move |_| Ok(staking_address));
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller
        .expect_get_block_denunciations()
        .returning(|slot, _timeout| {
            assert_eq!(*slot, Slot::new(1, 0));
            Ok(vec![])
        });
    let storage = Storage::create_root();
    let keypair_clone = keypair.clone();
    let mut pool_storage = storage.clone_without_refs();
    pool_controller
        .expect_get_block_operations()
        .returning(move |slot, _timeout| {
            assert_eq!(*slot, Slot::new(1, 0));
            let content = Operation {
                fee: Amount::from_str("0.01").unwrap(),
                expire_period: 2,
                op: OperationType::RollBuy { roll_count: 1 },
            };
            let operation = Operation::new_verifiable(
                content,
                OperationSerializer::new(),
                &keypair_clone,
                *CHAINID,
            )
            .unwrap();
            pool_storage.store_operations(vec![operation.clone()]);
            Ok((vec![operation.id], pool_storage.clone()))
        });
    let pool_storage_2 = storage.clone_without_refs();
    pool_controller
        .expect_get_block_endorsements()
        .returning(move |_, slot, _timeout| {
            assert_eq!(*slot, Slot::new(1, 0));
            Ok((vec![], pool_storage_2.clone()))
        });
    let pair = Arc::new((Mutex::new(false), Condvar::new()));
    let pair2 = pair.clone();
    let mut consensus_controller = Box::new(MockConsensusController::new());
    consensus_controller
        .expect_get_best_parents()
        .times(1)
        .return_once(move || parents);
    consensus_controller
        .expect_register_block()
        .times(1)
        .return_once(move |_, _, storage, created| {
            assert!(created);
            let block = storage.get_block_refs();
            assert_eq!(block.len(), 1);
            let ops = storage.get_op_refs();
            assert_eq!(ops.len(), 1);
            let (lock, cvar) = &*pair2;
            let mut started = lock.lock();
            *started = true;
            cvar.notify_one();
        });
    let mut test_factory = BlockTestFactory::new(
        &keypair,
        storage,
        consensus_controller,
        selector_controller,
        pool_controller,
    );
    let (lock, cvar) = &*pair;
    let mut started = lock.lock();
    if !*started {
        cvar.wait(&mut started);
    }
    test_factory.stop();
}

/// Test that the block factory produces a block even when all pools timeout.
/// This tests the resilience of the block production system - it should never
/// stop producing blocks just because pools are unresponsive.
#[test]
#[serial]
fn test_block_production_with_all_pools_timeout() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));
    let keypair = KeyPair::generate(0).unwrap();
    let storage = Storage::create_root();
    let staking_address = Address::from_public_key(&keypair.get_public_key());
    let parent = BlockId::generate_from_hash(Hash::compute_from("test".as_bytes()));
    let mut parents = Vec::new();
    for i in 0..THREAD_COUNT as u64 {
        parents.push((parent, i));
    }
    let pair = Arc::new((Mutex::new(false), Condvar::new()));
    let pair2 = pair.clone();

    // Setup consensus controller
    let mut consensus_controller = Box::new(MockConsensusController::new());
    consensus_controller
        .expect_get_best_parents()
        .times(1)
        .return_once(move || parents);
    consensus_controller
        .expect_register_block()
        .times(1)
        .return_once(move |_, _, storage, created| {
            assert!(created);
            let block_refs = storage.get_block_refs();
            assert_eq!(
                block_refs.len(),
                1,
                "Expected exactly one block to be created"
            );

            // Get the block to verify its contents
            let blocks = storage.read_blocks();
            let block = blocks.get(block_refs.iter().next().unwrap()).unwrap();

            // Verify that the block has no operations, endorsements, or denunciations
            assert_eq!(
                block.content.operations.len(),
                0,
                "Expected no operations in block when operation pool times out"
            );
            assert_eq!(
                block.content.header.content.endorsements.len(),
                0,
                "Expected no endorsements in block when endorsement pool times out"
            );
            assert_eq!(
                block.content.header.content.denunciations.len(),
                0,
                "Expected no denunciations in block when denunciation pool times out"
            );

            let (lock, cvar) = &*pair2;
            let mut started = lock.lock();
            *started = true;
            cvar.notify_one();
        });

    // Setup selector controller
    let mut selector_controller = Box::new(MockSelectorController::new());
    selector_controller
        .expect_get_producer()
        .times(1)
        .return_once(move |_| Ok(staking_address));

    // Setup pool controller that times out on all operations
    use massa_pool_exports::PoolError;
    let mut pool_controller = Box::new(MockPoolController::new());

    // All pool methods return timeout errors
    pool_controller
        .expect_get_block_denunciations()
        .returning(|slot, _timeout| {
            assert_eq!(*slot, Slot::new(1, 0));
            Err(PoolError::LockTimeout)
        });
    pool_controller
        .expect_get_block_operations()
        .returning(|slot, _timeout| {
            assert_eq!(*slot, Slot::new(1, 0));
            Err(PoolError::LockTimeout)
        });
    pool_controller
        .expect_get_block_endorsements()
        .returning(|_, slot, _timeout| {
            assert_eq!(*slot, Slot::new(1, 0));
            Err(PoolError::LockTimeout)
        });

    // Create and run the test factory
    let mut test_factory = BlockTestFactory::new(
        &keypair,
        storage,
        consensus_controller,
        selector_controller,
        pool_controller,
    );

    // Wait for the block to be created
    let (ref lock, ref cvar) = *pair;
    let mut started = lock.lock();
    if !*started {
        cvar.wait(&mut started);
    }

    test_factory.stop();
}
