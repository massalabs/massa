use std::time::Duration;

use massa_consensus_exports::ConsensusConfig;
use massa_execution_exports::MockExecutionController;
use massa_models::{
    address::Address, block::BlockGraphStatus, config::ENDORSEMENT_COUNT, slot::Slot,
};
use massa_pool_exports::MockPoolController;
use massa_pos_exports::{MockSelectorController, Selection};
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_time::MassaTime;

use super::tools::{consensus_test, create_block, register_block};

#[test]
fn test_process_refactor_logic() -> Result<(), Box<dyn std::error::Error>> {
    let t0_millis: u64 = 200;
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();

    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(t0_millis),
        thread_count: 2,
        genesis_timestamp: MassaTime::now(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 32,
        ..ConsensusConfig::default()
    };
    let storage = Storage::create_root();
    let staking_address = Address::from_public_key(&staking_key.get_public_key());

    // Mocks
    let mut execution_controller = Box::new(MockExecutionController::new());
    execution_controller
        .expect_update_blockclique_status()
        .returning(|_, _, _| {});

    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller
        .expect_notify_final_cs_periods()
        .returning(|_| {});
    pool_controller
        .expect_add_denunciation_precursor()
        .returning(|_| {});

    let mut selector_controller = Box::new(MockSelectorController::new());
    selector_controller
        .expect_get_producer()
        .returning(move |_| Ok(staking_address));
    selector_controller
        .expect_get_selection()
        .returning(move |_| {
            Ok(Selection {
                producer: staking_address,
                endorsements: vec![staking_address; ENDORSEMENT_COUNT as usize],
            })
        });

    consensus_test(
        cfg.clone(),
        execution_controller,
        pool_controller,
        selector_controller,
        move |consensus_controller| {
            let genesis = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status")
                .genesis_blocks;

            std::thread::sleep(Duration::from_millis(t0_millis));

            // Future block: should wait for slot.
            let future_slot = Slot::new(10, 0);
            let future_block =
                create_block(future_slot, vec![genesis[0], genesis[1]], &staking_key);

            register_block(&consensus_controller, future_block.clone(), storage.clone());

            // Missing dependency case.
            let block_1 = create_block(Slot::new(1, 0), vec![genesis[0], genesis[1]], &staking_key);
            let block_2 = create_block(Slot::new(1, 1), vec![block_1.id, genesis[1]], &staking_key);

            register_block(&consensus_controller, block_2.clone(), storage.clone());

            // Register parent so block_2 can be processed.
            register_block(&consensus_controller, block_1.clone(), storage.clone());

            // Wait for propagation.
            std::thread::sleep(Duration::from_millis(200));

            let statuses = consensus_controller.get_block_statuses(&[block_1.id, block_2.id]);

            assert_eq!(
                statuses,
                vec![
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "Block 2 should have been processed after Block 1 arrived"
            );
        },
    );
    Ok(())
}
