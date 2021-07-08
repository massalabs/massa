//RUST_BACKTRACE=1 cargo test scenarios106 -- --nocapture

use std::collections::HashMap;

use super::{
    mock_pool_controller::{MockPoolController, PoolCommandSink},
    mock_protocol_controller::MockProtocolController,
    tools,
};
use crate::{pos::RollCounts, start_consensus_controller, tests::tools::generate_ledger_file};
use models::Slot;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn test_consensus_sends_block_to_peer_who_asked_for_it() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<crypto::signature::PrivateKey> = (0..1)
        .map(|_| crypto::generate_random_private_key())
        .collect();
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let mut cfg = tools::default_consensus_config(
        2,
        ledger_file.path(),
        roll_counts_file.path(),
        staking_keys.clone(),
    );
    cfg.t0 = 1000.into();
    cfg.future_block_processing_max_periods = 50;
    cfg.max_future_processing_blocks = 10;

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let (pool_controller, pool_command_sender) = MockPoolController::new();
    let pool_sink = PoolCommandSink::new(pool_controller).await;

    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
            pool_command_sender,
            None,
            None,
            None,
            0,
        )
        .await
        .expect("could not start consensus controller");

    let start_slot = 3;
    let genesis_hashes = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    //create test blocks
    let slot = Slot::new(1 + start_slot, 0);
    let draw = consensus_command_sender
        .get_selection_draws(slot.clone(), Slot::new(2 + start_slot, 0))
        .await
        .expect("could not get selection draws.")[0]
        .1;
    let creator = tools::get_creator_for_draw(&draw, &cfg.staking_keys);
    let (hasht0s1, t0s1, _) = tools::create_block(
        &cfg,
        Slot::new(1 + start_slot, 0),
        genesis_hashes.clone(),
        creator,
    );

    // Send the actual block.
    protocol_controller.receive_block(t0s1).await;

    //block t0s1 is propagated
    let hash_list = vec![hasht0s1];
    tools::validate_propagate_block_in_list(
        &mut protocol_controller,
        &hash_list,
        3000 + start_slot as u64 * 1000,
    )
    .await;

    // Ask for the block to consensus.
    protocol_controller
        .receive_get_active_blocks(vec![hasht0s1])
        .await;

    // Consensus should respond with results including the block.
    tools::validate_block_found(&mut protocol_controller, &hasht0s1, 100).await;

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
    pool_sink.stop().await;
}

#[tokio::test]
#[serial]
async fn test_consensus_block_not_found() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<crypto::signature::PrivateKey> = (0..1)
        .map(|_| crypto::generate_random_private_key())
        .collect();
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let mut cfg = tools::default_consensus_config(
        2,
        ledger_file.path(),
        roll_counts_file.path(),
        staking_keys.clone(),
    );
    cfg.t0 = 1000.into();
    cfg.future_block_processing_max_periods = 50;
    cfg.max_future_processing_blocks = 10;

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let (pool_controller, pool_command_sender) = MockPoolController::new();
    let pool_sink = PoolCommandSink::new(pool_controller).await;

    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
            pool_command_sender,
            None,
            None,
            None,
            0,
        )
        .await
        .expect("could not start consensus controller");

    let start_slot = 3;
    let genesis_hashes = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    //create test blocks
    let (hasht0s1, _, _) = tools::create_block(
        &cfg,
        Slot::new(1 + start_slot, 0),
        genesis_hashes.clone(),
        cfg.staking_keys[0].clone(),
    );

    // Ask for the block to consensus.
    protocol_controller
        .receive_get_active_blocks(vec![hasht0s1])
        .await;

    // Consensus should not have the block.
    tools::validate_block_not_found(&mut protocol_controller, &hasht0s1, 100).await;

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
    pool_sink.stop().await;
}
