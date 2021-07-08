//RUST_BACKTRACE=1 cargo test scenarios106 -- --nocapture

use super::{
    mock_pool_controller::{MockPoolController, PoolCommandSink},
    mock_protocol_controller::MockProtocolController,
    tools,
};
use crate::{pos::RollCounts, start_consensus_controller, tests::tools::generate_ledger_file};
use models::Slot;
use serial_test::serial;
use std::collections::{HashMap, HashSet};
use std::iter::FromIterator;

#[tokio::test]
#[serial]
async fn test_wishlist_delta_with_empty_remove() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<crypto::signature::PrivateKey> = (0..2)
        .map(|_| crypto::generate_random_private_key())
        .collect();
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let mut cfg = tools::default_consensus_config(
        1,
        ledger_file.path(),
        roll_counts_file.path(),
        staking_keys.clone(),
    );
    cfg.t0 = 500.into();
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
            0,
        )
        .await
        .expect("could not start consensus controller");

    let genesis_hashes = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    //create test blocks
    let slot = Slot::new(1, 0);
    let draw = consensus_command_sender
        .get_selection_draws(slot.clone(), Slot::new(2, 0))
        .await
        .expect("could not get selection draws.")[0]
        .1;
    let creator = tools::get_creator_for_draw(&draw, &cfg.staking_keys);
    let (hasht0s1, t0s1, _) =
        tools::create_block(&cfg, Slot::new(1, 0), genesis_hashes.clone(), creator);

    //send header for block t0s1
    protocol_controller
        .receive_header(t0s1.header.clone())
        .await;

    let expected_new = HashSet::from_iter(vec![hasht0s1].into_iter());
    let expected_remove = HashSet::from_iter(vec![].into_iter());
    tools::validate_wishlist(
        &mut protocol_controller,
        expected_new,
        expected_remove,
        1000,
    )
    .await;

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
async fn test_wishlist_delta_remove() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<crypto::signature::PrivateKey> = (0..2)
        .map(|_| crypto::generate_random_private_key())
        .collect();
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let mut cfg = tools::default_consensus_config(
        1,
        ledger_file.path(),
        roll_counts_file.path(),
        staking_keys.clone(),
    );
    cfg.t0 = 500.into();
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
            0,
        )
        .await
        .expect("could not start consensus controller");

    let genesis_hashes = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    //create test blocks
    let (hasht0s1, t0s1, _) = tools::create_block(
        &cfg,
        Slot::new(1, 0),
        genesis_hashes.clone(),
        cfg.staking_keys[0].clone(),
    );
    //send header for block t0s1
    protocol_controller
        .receive_header(t0s1.header.clone())
        .await;

    let expected_new = HashSet::from_iter(vec![hasht0s1].into_iter());
    let expected_remove = HashSet::from_iter(vec![].into_iter());
    tools::validate_wishlist(
        &mut protocol_controller,
        expected_new,
        expected_remove,
        1000,
    )
    .await;

    protocol_controller.receive_block(t0s1.clone()).await;
    let expected_new = HashSet::from_iter(vec![].into_iter());
    let expected_remove = HashSet::from_iter(vec![hasht0s1].into_iter());
    tools::validate_wishlist(
        &mut protocol_controller,
        expected_new,
        expected_remove,
        1000,
    )
    .await;

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
    pool_sink.stop().await;
}
