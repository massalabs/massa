use std::sync::Arc;

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
        .returning(|slot| {
            assert_eq!(*slot, Slot::new(1, 0));
            vec![]
        });
    pool_controller
        .expect_get_block_operations()
        .returning(|slot| {
            assert_eq!(*slot, Slot::new(1, 0));
            (vec![], Storage::create_root())
        });
    pool_controller
        .expect_get_block_endorsements()
        .returning(|_, slot| {
            assert_eq!(*slot, Slot::new(1, 0));
            (vec![], Storage::create_root())
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
        .returning(|slot| {
            assert_eq!(*slot, Slot::new(1, 0));
            vec![]
        });
    let storage = Storage::create_root();
    let keypair_clone = keypair.clone();
    let mut pool_storage = storage.clone_without_refs();
    pool_controller
        .expect_get_block_operations()
        .returning(move |slot| {
            assert_eq!(*slot, Slot::new(1, 0));
            let content = Operation {
                fee: Amount::from_raw(10000000000000000),
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
            (vec![operation.id], pool_storage.clone())
        });
    let pool_storage_2 = storage.clone_without_refs();
    pool_controller
        .expect_get_block_endorsements()
        .returning(move |_, slot| {
            assert_eq!(*slot, Slot::new(1, 0));
            (vec![], pool_storage_2.clone())
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
