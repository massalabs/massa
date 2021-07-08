use std::collections::HashMap;

use super::{
    mock_pool_controller::{MockPoolController, PoolCommandSink},
    mock_protocol_controller::MockProtocolController,
    tools,
};
use crate::{pos::RollCounts, start_consensus_controller, tests::tools::generate_ledger_file};
use models::Slot;
use serial_test::serial;
use time::UTime;

//create 2 clique. Extend the first until the second is disgarded.
//verify the disgarded block are in storage.
//verify that genesis and other click blocks aren't in storage.
#[tokio::test]
#[serial]
async fn test_storage() {
    // setup logging
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/
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
    cfg.t0 = 32000.into();
    cfg.delta_f0 = 10;
    cfg.max_discarded_blocks = 1;

    // to avoid timing problems for blocks in the future
    cfg.genesis_timestamp = UTime::now(0)
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(1000).unwrap());

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let (pool_controller, pool_command_sender) = MockPoolController::new();
    let pool_sink = PoolCommandSink::new(pool_controller).await;

    // start storage
    let storage_access = tools::start_storage();

    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
            pool_command_sender,
            Some(storage_access.clone()),
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

    //create a valids block for thread 0
    let valid_hasht0s1 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        Slot::new(1, 0),
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;
    //create a valid block on the other thread.
    let valid_hasht1s1 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        Slot::new(1, 1),
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    //Create other clique bock T0S2
    let fork_block_hash = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        Slot::new(2, 0),
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    assert!(!&storage_access.contains(fork_block_hash).await.unwrap());

    //extend first clique
    let mut parentt0sn_hash = valid_hasht0s1;
    let mut parentt1sn_hash = valid_hasht1s1;
    for period in 3..=12 {
        let block_hash_0 = tools::create_and_test_block(
            &mut protocol_controller,
            &cfg,
            Slot::new(period, 0),
            vec![parentt0sn_hash, parentt1sn_hash],
            true,
            false,
        )
        .await;
        parentt0sn_hash = block_hash_0;

        let block_hash = tools::create_and_test_block(
            &mut protocol_controller,
            &cfg,
            Slot::new(period, 1),
            vec![parentt0sn_hash, parentt1sn_hash],
            true,
            false,
        )
        .await;
        parentt1sn_hash = block_hash;
    }

    assert!(!&storage_access.contains(fork_block_hash).await.unwrap());
    assert!(&storage_access.contains(genesis_hashes[0]).await.unwrap());
    assert!(&storage_access.contains(genesis_hashes[1]).await.unwrap());
    assert!(&storage_access.contains(valid_hasht0s1).await.unwrap());

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
    pool_sink.stop().await;
}
