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

use mockall::Sequence;
use std::rc::Rc;
use std::time::Duration;

use crate::tests::tools::create_some_operations;
use crate::tests::tools::OpGenerator;
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
use massa_pool_exports::PoolConfig;
use massa_pos_exports::MockSelectorController;
use massa_pos_exports::{PosResult, Selection};
use massa_signature::KeyPair;

use super::tools::PoolTestBoilerPlate;

/// # Test simple get operation
/// Just try to get some operations stored in pool
///
/// ## Initialization
/// Insert multiple operations in the pool. (10)
///
/// Create a mock-execution-controller story:
/// 1. unexpected_opse_among, returning the op-ids of what gets inserted into the pool
/// 2. get_final_and_candidate_balance, returning 1, 1
/// 3. repeat #1 9 times
///
/// The block operation storage built for all threads is expected to have the
/// same length than those added previously.
#[test]
fn test_simple_get_operations() {
    // Setup the execution story.
    let keypair = KeyPair::generate();
    let addr = Address::from_public_key(&keypair.get_public_key()).clone();

    // setup operations
    let op_gen = OpGenerator::default().creator(keypair.clone()).expirery(1);
    let ops = create_some_operations(10, &op_gen);
    let mock_owned_ops = ops.iter().map(|op| op.id).collect();

    let mut execution_controller = Box::new(MockExecutionController::new());
    execution_controller.expect_clone_box().returning(move || {
        Box::new(create_basic_get_block_operation_execution_mock(
            10,
            addr,
            vec![(Some(Amount::from_raw(1)), Some(Amount::from_raw(1)))],
            &mock_owned_ops,
        ))
    });

    // Provide the selector boilderplate
    let mut selector_controller = Box::new(MockSelectorController::new());
    selector_controller
        .expect_clone_box()
        .returning(|| Box::new(MockSelectorController::new()));

    // Setup the pool controller
    let config = PoolConfig::default();

    let PoolTestBoilerPlate {
        mut pool_manager,
        mut pool_controller,
        mut storage,
    } = PoolTestBoilerPlate::pool_test(config, execution_controller, selector_controller);

    // setup storage
    storage.store_operations(ops);
    pool_controller.add_operations(storage);

    // Allow some time for the pool to add the operations
    std::thread::sleep(Duration::from_millis(100));

    let creator_thread = {
        let creator_address = Address::from_public_key(&keypair.get_public_key());
        creator_address.get_thread(config.thread_count)
    };
    // This is what we are testing....
    let block_operations_storage = pool_controller
        .get_block_operations(&Slot::new(1, creator_thread))
        .1;

    pool_manager.stop();

    assert_eq!(block_operations_storage.get_op_refs().len(), 10);
}

/// Create default mock-story for execution controller on call `get_block_operation` API.
pub fn create_basic_get_block_operation_execution_mock(
    operations_len: usize,
    creator_address: Address,
    balance_vec: Vec<(Option<Amount>, Option<Amount>)>,
    ops: &PreHashSet<OperationId>,
) -> MockExecutionController {
    let mut res = MockExecutionController::new();
    let mut seq = Sequence::new();
    let ops1 = ops.clone();
    let ops2 = Rc::new(ops.clone());
    res.expect_unexecuted_ops_among()
        .times(1)
        .return_once(|_, _| ops1)
        .in_sequence(&mut seq);
    res.expect_get_final_and_candidate_balance()
        .times(1)
        .return_once(|_| balance_vec)
        .withf(move |addrs| addrs.len() == 1 && addrs[0] == creator_address)
        .in_sequence(&mut seq);
    res.expect_unexecuted_ops_among()
        .times(operations_len - 1)
        .returning_st(move |_, _| (&*ops2).clone())
        .in_sequence(&mut seq);
    res
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
    let keypair = KeyPair::generate();
    let creator_address = Address::from_public_key(&keypair.get_public_key());
    let op_gen = OpGenerator::default().expirery(1).creator(keypair);
    let operations = create_some_operations(OP_LEN, &op_gen);
    let max_block_size = operations
        .iter()
        .take(MAX_OP_LEN)
        .fold(0, |acc, op| acc + op.serialized_size() as u32);
    let config = PoolConfig {
        max_block_size,
        ..Default::default()
    };
    let creator_thread = creator_address.get_thread(config.thread_count);

    let mut execution_controller = Box::new(MockExecutionController::new());
    execution_controller.expect_clone_box().returning(move || {
        Box::new(create_basic_get_block_operation_execution_mock(
            MAX_OP_LEN,
            creator_address,
            vec![(Some(Amount::from_raw(1)), Some(Amount::from_raw(1)))],
            &operations.iter().take(MAX_OP_LEN).map(|op| op.id).collect(),
        ))
    });

    // Provide the selector boilderplate
    let mut selector_controller = Box::new(MockSelectorController::new());
    selector_controller
        .expect_clone_box()
        .returning(|| Box::new(MockSelectorController::new()));

    let PoolTestBoilerPlate {
        mut pool_manager,
        mut pool_controller,
        mut storage,
    } = PoolTestBoilerPlate::pool_test(config, execution_controller, selector_controller);

    storage.store_operations(create_some_operations(10, &op_gen));
    pool_controller.add_operations(storage);
    // Allow some time for the pool to add the operations
    std::thread::sleep(Duration::from_millis(100));

    // This is what we are testing....
    let block_operations_storage = pool_controller
        .get_block_operations(&Slot::new(1, creator_thread))
        .1;
    pool_manager.stop();

    assert_eq!(block_operations_storage.get_op_refs().len(), MAX_OP_LEN);
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
    // let mut selector_controller = Box::new(MockSelectorController::new());
    let mut res = MockSelectorController::new();
    res.expect_get_producer()
        .times(2)
        .returning(move |_| PosResult::Ok(address));
    let selector_controller = pool_test_mock_selector_controller(res);

    let mut execution_controller = Box::new(MockExecutionController::new());
    execution_controller
        .expect_clone_box()
        .returning(move || Box::new(MockExecutionController::new()));
    let PoolTestBoilerPlate {
        mut pool_manager,
        pool_controller,
        storage: _storage,
    } = PoolTestBoilerPlate::pool_test(config, execution_controller, selector_controller);

    pool_controller.add_denunciation_precursor(de_p_1);
    pool_controller.add_denunciation_precursor(de_p_2);
    std::thread::sleep(Duration::from_millis(100));

    assert_eq!(pool_controller.get_denunciation_count(), 1);
    assert_eq!(
        pool_controller.contains_denunciation(&denunciation_orig),
        true
    );

    pool_manager.stop();
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
    {
        let mut execution_controller = Box::new(MockExecutionController::new());
        execution_controller
            .expect_clone_box()
            .returning(move || Box::new(MockExecutionController::new()));

        let mut res = MockSelectorController::new();
        res.expect_get_selection().times(2).returning(move |_| {
            PosResult::Ok(Selection {
                endorsements: vec![address; usize::from(config.thread_count)],
                producer: address,
            })
        });
        let selector_controller = pool_test_mock_selector_controller(res);
        let PoolTestBoilerPlate {
            mut pool_manager,
            pool_controller,
            storage: _storage,
        } = PoolTestBoilerPlate::pool_test(config, execution_controller, selector_controller);

        {
            pool_controller.add_denunciation_precursor(de_p_1);
            pool_controller.add_denunciation_precursor(de_p_2);
            std::thread::sleep(Duration::from_millis(200));

            assert_eq!(pool_controller.get_denunciation_count(), 1);
            assert_eq!(
                pool_controller.contains_denunciation(&denunciation_orig),
                true
            );

            pool_manager.stop();
        }
    };
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
    let execution_controller = {
        let mut res = Box::new(MockExecutionController::new());
        res.expect_clone_box()
            .return_once(move || Box::new(MockExecutionController::new()));

        res.expect_is_denunciation_executed()
            .times(2)
            .returning(move |de_idx| de_idx != &de_idx_2);
        res
    };

    let selector_controller = {
        let mut res = MockSelectorController::new();
        res.expect_get_producer()
            .times(2)
            .returning(move |_| PosResult::Ok(address_2));
        res.expect_get_selection().times(2).returning(move |_| {
            PosResult::Ok(Selection {
                endorsements: vec![address_1; usize::from(config.thread_count)],
                producer: address_1,
            })
        });
        pool_test_mock_selector_controller(res)
    };

    let PoolTestBoilerPlate {
        mut pool_manager,
        pool_controller,
        storage: _storage,
    } = PoolTestBoilerPlate::pool_test(config, execution_controller, selector_controller);

    // And so begins the test
    {
        // ~ random order (but need to keep the precursor order otherwise Denunciation::PartialEq will fail)
        pool_controller.add_denunciation_precursor(de_p_3);
        pool_controller.add_denunciation_precursor(de_p_1);
        pool_controller.add_denunciation_precursor(de_p_4);
        pool_controller.add_denunciation_precursor(de_p_2);

        std::thread::sleep(Duration::from_millis(200));

        assert_eq!(pool_controller.get_denunciation_count(), 2);
        assert_eq!(
            pool_controller.contains_denunciation(&denunciation_orig_1),
            true
        );
        assert_eq!(
            pool_controller.contains_denunciation(&denunciation_orig_2),
            true
        );

        let target_slot_1 = Slot::new(4, 0);

        let denunciations = pool_controller.get_block_denunciations(&target_slot_1);

        assert_eq!(denunciations, vec![denunciation_orig_2]);

        pool_manager.stop();
    }
}

// The _actual_ story of the mock involves some clones that we don't want to worry about.
// This helper method means that tests need only concern themselves with the actual story.
pub fn pool_test_mock_selector_controller(
    story: MockSelectorController,
) -> Box<MockSelectorController> {
    let mut selector_controller = Box::new(MockSelectorController::new());
    selector_controller
        .expect_clone_box()
        .times(2)
        .returning(move || Box::new(MockSelectorController::new()));
    selector_controller
        .expect_clone_box()
        .return_once(move || Box::new(story));
    selector_controller
}
