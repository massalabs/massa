use super::{mock_protocol_controller::MockProtocolController, tools};
use crate::start_consensus_controller;
use models::Slot;
use time::UTime;

//create 2 clique. Extend the first until the second is disgarded.
//verify the disgarded block are in storage.
//verify that genesis and other click blocks aren't in storage.
#[tokio::test]
async fn test_storage() {
    // setup logging
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/

    let (mut cfg, serialization_context) = tools::default_consensus_config(1);
    cfg.t0 = 32000.into();
    cfg.delta_f0 = 10;
    cfg.max_discarded_blocks = 1;

    // to avoid timing problems for blocks in the future
    cfg.genesis_timestamp = UTime::now(0)
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(1000).unwrap());

    // mock protocol
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new(serialization_context.clone());

    // start storage
    let storage_access = tools::start_storage(&serialization_context);

    // launch consensus controller
    let (consensus_command_sender, _consensus_event_receiver, _consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            serialization_context.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
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
        &serialization_context,
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
        &serialization_context,
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
        &serialization_context,
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
    for period in 3..=11 {
        let block_hash_0 = tools::create_and_test_block(
            &mut protocol_controller,
            &cfg,
            &serialization_context,
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
            &serialization_context,
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
}
