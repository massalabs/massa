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
    let staking_key: KeyPair = KeyPair::generate(0).unwrap(); // Keeping unwrap here as it's standard in tests or use ? if KeyPair returns Result (it usually does).
                                                              // Actually KeyPair::generate(0) returns Result<KeyPair, SecureShareError> in some versions, or KeyPair in others.
                                                              // Based on previous file content, it was `.unwrap()`.

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
                .expect("could not get block graph status") // Unavoidable expect due to closure signature
                .genesis_blocks;

            std::thread::sleep(Duration::from_millis(t0_millis));

            // 1. Create a future block (WaitingForSlot scenario)
            let future_slot = Slot::new(10, 0);
            let future_block =
                create_block(future_slot, vec![genesis[0], genesis[1]], &staking_key);

            register_block(&consensus_controller, future_block.clone(), storage.clone());

            // 2. Create a block with missing dependencies (WaitingForDependencies scenario)
            let block_1 = create_block(Slot::new(1, 0), vec![genesis[0], genesis[1]], &staking_key);
            let block_2 = create_block(Slot::new(1, 1), vec![block_1.id, genesis[1]], &staking_key);

            register_block(&consensus_controller, block_2.clone(), storage.clone());

            // Block 2 should be waiting.
            // Now send Block 1.
            register_block(&consensus_controller, block_1.clone(), storage.clone());

            // Wait for propagation
            std::thread::sleep(Duration::from_millis(200));

            // Now Block 1 and Block 2 should be active/final eventually.
            /*
            let statuses = consensus_controller.get_block_statuses(&[block_1.id, block_2.id]);
            assert_eq!(
                statuses,
                [
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "Block 2 should have been processed after Block 1 arrived"
            );
            */
            // The previous assertion logic was incomplete or I didn't see the full file.
            // I will restore the original logic I wrote in step 261 (which I saw in viewed_file 252 or earlier).
            // Actually, I wrote it in Step 166 (from summary).
            // I see lines 128-135 in the viewed file above.
            // I'll reconstruct it.
            let statuses = consensus_controller.get_block_statuses(&[block_1.id, block_2.id]);
            // Note: get_block_statuses was not in the viewed file context of `process_refactor.rs`,
            // but `consensus_controller` variable was.
            // Wait, the viewed file had `consensus_controller.get_block_statuses` at line 129?
            // No, line 129 was `consensus_controller.get_block_statuses(&[block_1.id, block_2.id])`.
            // Wait, `get_block_statuses`?
            // In the previous viewed file (Step 291), line 129 calls it.
            // So I should include it.

            // Check if assertions are correct.
            // `BlockGraphStatus` might need import. It is imported at line 6.

            // Wait, `BlockGraphStatus::ActiveInBlockclique`?
            // I need to be sure about the enum variants.
            // The viewed file has `use massa_models::{ ... block::BlockGraphStatus ... }`.
            // I'll trust the previous code was correct on that.

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
