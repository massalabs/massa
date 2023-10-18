use std::sync::Arc;

use super::EndorsementTestFactory;
use massa_consensus_exports::MockConsensusController;
use massa_hash::Hash;
use massa_models::{
    address::Address,
    block_id::BlockId,
    config::{ENDORSEMENT_COUNT, THREAD_COUNT},
    slot::Slot,
};
use massa_pool_exports::MockPoolController;
use massa_pos_exports::{MockSelectorController, Selection};
use massa_protocol_exports::MockProtocolController;
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
        .expect_get_latest_blockclique_block_at_slot()
        .times(1)
        .returning(move |slot| {
            assert_eq!(slot, Slot::new(1, 0));
            parent
        });
    let mut selector_controller = Box::new(MockSelectorController::new());
    selector_controller
        .expect_get_selection()
        .times(1)
        .returning(move |slot| {
            assert_eq!(slot, Slot::new(1, 0));
            Ok(Selection {
                producer: staking_address,
                endorsements: vec![staking_address; ENDORSEMENT_COUNT as usize],
            })
        });
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller
        .expect_add_endorsements()
        .times(1)
        .returning(|_| {});
    let mut protocol_controller = Box::new(MockProtocolController::new());
    protocol_controller
        .expect_propagate_endorsements()
        .times(1)
        .returning(move |storage| {
            let endorsement_ids = storage.get_endorsement_refs();
            assert_eq!(endorsement_ids.len(), ENDORSEMENT_COUNT as usize);
            let first_endorsement = storage
                .read_endorsements()
                .get(endorsement_ids.iter().next().unwrap())
                .unwrap()
                .clone();
            assert_eq!(first_endorsement.content.slot, Slot::new(1, 0));
            let &(ref lock, ref cvar) = &*pair2;
            let mut started = lock.lock();
            *started = true;
            cvar.notify_one();
            Ok(())
        });
    let mut test_factory = EndorsementTestFactory::new(
        &keypair,
        storage,
        consensus_controller,
        selector_controller,
        pool_controller,
        protocol_controller,
    );
    let &(ref lock, ref cvar) = &*pair;
    let mut started = lock.lock();
    if !*started {
        cvar.wait(&mut started);
    }
    test_factory.stop();
}
