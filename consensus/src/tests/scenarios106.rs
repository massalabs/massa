//RUST_BACKTRACE=1 cargo test scenarios106 -- --nocapture

use super::super::{
    consensus_controller::ConsensusController,
    default_consensus_controller::DefaultConsensusController, timeslots,
};
use super::mock_protocol_controller::{self};
use super::tools;
use crypto::hash::Hash;
use std::collections::HashSet;
use time::UTime;

#[tokio::test]
async fn test_unsorted_block() {
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/
    let node_ids = tools::create_node_ids(1);

    let (protocol_controller, mut protocol_controler_interface) = mock_protocol_controller::new();
    let mut cfg = tools::default_consensus_config(&node_ids);
    cfg.t0 = 1000.into();
    cfg.future_block_processing_max_periods = 50;
    cfg.max_future_processing_blocks = 10;

    let cnss = DefaultConsensusController::new(&cfg, protocol_controller)
        .await
        .expect("Could not create consensus controller");

    let start_slot = 3;
    let genesis_hashes = cnss
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;
    //create test blocks

    let (hasht0s1, t0s1, _) = tools::create_block(&cfg, 0, 1 + start_slot, genesis_hashes.clone());

    let (hasht1s1, t1s1, _) = tools::create_block(&cfg, 1, 1 + start_slot, genesis_hashes.clone());

    let (hasht0s2, t0s2, _) =
        tools::create_block(&cfg, 0, 2 + start_slot, vec![hasht0s1, hasht1s1]);
    let (hasht1s2, t1s2, _) =
        tools::create_block(&cfg, 1, 2 + start_slot, vec![hasht0s1, hasht1s1]);

    let (hasht0s3, t0s3, _) =
        tools::create_block(&cfg, 0, 3 + start_slot, vec![hasht0s2, hasht1s2]);
    let (hasht1s3, t1s3, _) =
        tools::create_block(&cfg, 1, 3 + start_slot, vec![hasht0s2, hasht1s2]);

    let (hasht0s4, t0s4, _) =
        tools::create_block(&cfg, 0, 4 + start_slot, vec![hasht0s3, hasht1s3]);
    let (hasht1s4, t1s4, _) =
        tools::create_block(&cfg, 1, 4 + start_slot, vec![hasht0s3, hasht1s3]);

    //send blocks  t0s1, t1s1,
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t0s1)
        .await;
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t1s1)
        .await;
    //send blocks t0s3, t1s4, t0s4, t0s2, t1s3, t1s2
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t0s3)
        .await;
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t1s4)
        .await;
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t0s4)
        .await;
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t0s2)
        .await;
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t1s3)
        .await;
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t1s2)
        .await;

    //block t0s1 and t1s1 are propagated
    let hash_list = vec![hasht0s1, hasht1s1];
    tools::validate_propagate_block_in_list(
        &mut protocol_controler_interface,
        &hash_list,
        3000 + start_slot * 1000,
    )
    .await;
    tools::validate_propagate_block_in_list(&mut protocol_controler_interface, &hash_list, 1000)
        .await;
    //block t0s2 and t1s2 are propagated
    let hash_list = vec![hasht0s2, hasht1s2];
    tools::validate_propagate_block_in_list(&mut protocol_controler_interface, &hash_list, 1000)
        .await;
    tools::validate_propagate_block_in_list(&mut protocol_controler_interface, &hash_list, 1000)
        .await;
    //block t0s3 and t1s3 are propagated
    let hash_list = vec![hasht0s3, hasht1s3];
    tools::validate_propagate_block_in_list(&mut protocol_controler_interface, &hash_list, 1000)
        .await;
    tools::validate_propagate_block_in_list(&mut protocol_controler_interface, &hash_list, 1000)
        .await;
    //block t0s4 and t1s4 are propagated
    let hash_list = vec![hasht0s4, hasht1s4];
    tools::validate_propagate_block_in_list(&mut protocol_controler_interface, &hash_list, 1000)
        .await;
    tools::validate_propagate_block_in_list(&mut protocol_controler_interface, &hash_list, 4000)
        .await;
}

//test future_incoming_blocks block in the future with max_future_processing_blocks.
#[tokio::test]
async fn test_unsorted_block_with_to_much_in_the_future() {
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/
    let node_ids = tools::create_node_ids(1);
    let (protocol_controller, mut protocol_controler_interface) = mock_protocol_controller::new();
    let mut cfg = tools::default_consensus_config(&node_ids);
    cfg.t0 = 1000.into();
    cfg.genesis_timestamp = UTime::now().unwrap().saturating_sub(2000.into()); // slot 1 is in the past
    cfg.future_block_processing_max_periods = 3;
    cfg.max_future_processing_blocks = 5;
    let cnss = DefaultConsensusController::new(&cfg, protocol_controller)
        .await
        .expect("could not create consensus controller");

    //create test blocks
    let genesis_hashes = cnss
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    // a block in the past must be propagated
    let (hash1, block1, _) = tools::create_block(&cfg, 0, 1, genesis_hashes.clone());
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &block1)
        .await;
    tools::validate_propagate_block(&mut protocol_controler_interface, hash1, 1000).await;

    // this block is slightly in the future: will wait for it
    let (last_period, last_thread) =
        timeslots::get_current_latest_block_slot(cfg.thread_count, cfg.t0, cfg.genesis_timestamp)
            .unwrap()
            .unwrap();
    let (hash2, block2, _) =
        tools::create_block(&cfg, last_thread, last_period + 2, genesis_hashes.clone());
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &block2)
        .await;
    assert!(
        !tools::validate_notpropagate_block(&mut protocol_controler_interface, hash2, 500).await
    );
    tools::validate_propagate_block(&mut protocol_controler_interface, hash2, 2500).await;

    // this block is too much in the future: do not process
    let (last_period, last_thread) =
        timeslots::get_current_latest_block_slot(cfg.thread_count, cfg.t0, cfg.genesis_timestamp)
            .unwrap()
            .unwrap();
    let (hash3, block3, _) = tools::create_block(
        &cfg,
        last_thread,
        last_period + 1000,
        genesis_hashes.clone(),
    );
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &block3)
        .await;
    assert!(
        !tools::validate_notpropagate_block(&mut protocol_controler_interface, hash3, 2500).await
    );

    //validate the block isn't final.
    let block_graph = cnss.get_block_graph_status().await.unwrap();
    assert!(!block_graph.discarded_blocks.set.contains(&hash3));
}

#[tokio::test]
async fn test_too_many_blocks_in_the_future() {
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/
    let node_ids = tools::create_node_ids(1);
    let (protocol_controller, mut protocol_controler_interface) = mock_protocol_controller::new();
    let mut cfg = tools::default_consensus_config(&node_ids);
    cfg.t0 = 1000.into();
    cfg.future_block_processing_max_periods = 100;
    cfg.max_future_processing_blocks = 2;
    cfg.delta_f0 = 1000;
    cfg.genesis_timestamp = UTime::now().unwrap().saturating_sub(2000.into()); // slot 1 is in the past
    let cnss = DefaultConsensusController::new(&cfg, protocol_controller)
        .await
        .expect("could not create consensus controller");

    //get genesis block hashes
    let genesis_hashes = cnss
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    // generate 5 blocks but there is only space for 2 in the waiting line
    let mut expected_block_hashes: HashSet<Hash> = HashSet::new();
    let mut max_period = 0;
    let (last_period, last_thread) =
        timeslots::get_current_latest_block_slot(cfg.thread_count, cfg.t0, cfg.genesis_timestamp)
            .unwrap()
            .unwrap();
    for period in 0..5 {
        max_period = last_period + 2 + period;
        let (hash, block, _) =
            tools::create_block(&cfg, last_thread, max_period, genesis_hashes.clone());
        protocol_controler_interface
            .receive_block(node_ids[0].1.clone(), &block)
            .await;
        if period < 2 {
            expected_block_hashes.insert(hash);
        }
    }
    // wait for the 2 waiting blocks to propagate
    let mut expected_clone = expected_block_hashes.clone();
    while !expected_block_hashes.is_empty() {
        assert!(
            expected_block_hashes.remove(
                &tools::validate_propagate_block_in_list(
                    &mut protocol_controler_interface,
                    &expected_block_hashes.iter().copied().collect(),
                    2500
                )
                .await
            ),
            "unexpected block propagated"
        );
    }
    // wait until we reach the slot of the last block
    while timeslots::get_current_latest_block_slot(cfg.thread_count, cfg.t0, cfg.genesis_timestamp)
        .unwrap()
        .unwrap()
        < (max_period + 1, 0)
    {}
    // ensure that the graph contains only what we expect
    let graph = cnss
        .get_block_graph_status()
        .await
        .expect("could not get block graph status");
    expected_clone.extend(graph.genesis_blocks);
    assert_eq!(
        expected_clone,
        graph.active_blocks.keys().copied().collect(),
        "unexpected block graph"
    );
}

#[tokio::test]
async fn test_dep_in_back_order() {
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/
    let node_ids = tools::create_node_ids(1);

    let (protocol_controller, mut protocol_controler_interface) = mock_protocol_controller::new();
    let mut cfg = tools::default_consensus_config(&node_ids);
    cfg.t0 = 1000.into();
    cfg.genesis_timestamp = UTime::now()
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(1000).unwrap());

    cfg.max_dependency_blocks = 10;

    let cnss = DefaultConsensusController::new(&cfg, protocol_controller)
        .await
        .expect("Could not create consensus controller");
    let genesis_hashes = cnss
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    //create test blocks
    let (hasht0s1, t0s1, _) = tools::create_block(&cfg, 0, 1, genesis_hashes.clone());

    let (hasht1s1, t1s1, _) = tools::create_block(&cfg, 1, 1, genesis_hashes.clone());

    let (hasht0s2, t0s2, _) = tools::create_block(&cfg, 0, 2, vec![hasht0s1, hasht1s1]);
    let (hasht1s2, t1s2, _) = tools::create_block(&cfg, 1, 2, vec![hasht0s1, hasht1s1]);

    let (hasht0s3, t0s3, _) = tools::create_block(&cfg, 0, 3, vec![hasht0s2, hasht1s2]);
    let (hasht1s3, t1s3, _) = tools::create_block(&cfg, 1, 3, vec![hasht0s2, hasht1s2]);

    let (hasht0s4, t0s4, _) = tools::create_block(&cfg, 0, 4, vec![hasht0s3, hasht1s3]);
    let (hasht1s4, t1s4, _) = tools::create_block(&cfg, 1, 4, vec![hasht0s3, hasht1s3]);

    //send blocks   t0s2, t1s3, t0s1, t0s4, t1s4, t1s1, t0s3, t1s2
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t0s2)
        .await;
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t1s3)
        .await;
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t0s1)
        .await;
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t0s4)
        .await;
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t1s4)
        .await;
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t1s1)
        .await;
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t0s3)
        .await;
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t1s2)
        .await;

    //block t0s1 and t1s1 are propagated
    let hash_list = vec![hasht0s1, hasht1s1];
    tools::validate_propagate_block_in_list(&mut protocol_controler_interface, &hash_list, 1000)
        .await;
    tools::validate_propagate_block_in_list(&mut protocol_controler_interface, &hash_list, 1000)
        .await;
    //block t0s2 and t1s2 are propagated
    let hash_list = vec![hasht0s2, hasht1s2];
    tools::validate_propagate_block_in_list(&mut protocol_controler_interface, &hash_list, 1000)
        .await;
    tools::validate_propagate_block_in_list(&mut protocol_controler_interface, &hash_list, 1000)
        .await;
    //block t0s3 and t1s3 are propagated
    let hash_list = vec![hasht0s3, hasht1s3];
    tools::validate_propagate_block_in_list(&mut protocol_controler_interface, &hash_list, 1000)
        .await;
    tools::validate_propagate_block_in_list(&mut protocol_controler_interface, &hash_list, 1000)
        .await;
    //block t0s4 and t1s4 are propagated
    let hash_list = vec![hasht0s4, hasht1s4];
    tools::validate_propagate_block_in_list(&mut protocol_controler_interface, &hash_list, 1000)
        .await;
    tools::validate_propagate_block_in_list(&mut protocol_controler_interface, &hash_list, 4000)
        .await;
}

#[tokio::test]
async fn test_dep_in_back_order_with_max_dependency_blocks() {
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/
    let node_ids = tools::create_node_ids(1);

    let (protocol_controller, mut protocol_controler_interface) = mock_protocol_controller::new();
    let mut cfg = tools::default_consensus_config(&node_ids);
    cfg.t0 = 1000.into();
    cfg.genesis_timestamp = UTime::now()
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(1000).unwrap());
    cfg.max_dependency_blocks = 2;

    let cnss = DefaultConsensusController::new(&cfg, protocol_controller)
        .await
        .expect("Could not create consensus controller");
    let genesis_hashes = cnss
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    //create test blocks

    let (hasht0s1, t0s1, _) = tools::create_block(&cfg, 0, 1, genesis_hashes.clone());

    let (hasht1s1, t1s1, _) = tools::create_block(&cfg, 1, 1, genesis_hashes.clone());

    let (hasht0s2, t0s2, _) = tools::create_block(&cfg, 0, 2, vec![hasht0s1, hasht1s1]);
    let (hasht1s2, t1s2, _) = tools::create_block(&cfg, 1, 2, vec![hasht0s1, hasht1s1]);

    let (hasht0s3, t0s3, _) = tools::create_block(&cfg, 0, 3, vec![hasht0s2, hasht1s2]);
    let (hasht1s3, t1s3, _) = tools::create_block(&cfg, 1, 3, vec![hasht0s2, hasht1s2]);

    //send blocks   t0s2, t1s3, t0s1, t0s4, t1s4, t1s1, t0s3, t1s2
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t0s2)
        .await;
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t1s3)
        .await;
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t0s1)
        .await;
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t0s3)
        .await;
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t1s2)
        .await;
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t1s1)
        .await;

    let mut expected_blocks: HashSet<Hash> =
        vec![hasht0s1, hasht1s1, hasht1s2].into_iter().collect();
    let unexpected_blocks: HashSet<Hash> = vec![hasht0s2, hasht0s3, hasht1s3].into_iter().collect();

    while !expected_blocks.is_empty() {
        expected_blocks.remove(
            &tools::validate_propagate_block_in_list(
                &mut protocol_controler_interface,
                &expected_blocks.iter().copied().collect(),
                1000,
            )
            .await,
        );
    }
    assert!(
        !tools::validate_notpropagate_block_in_list(
            &mut protocol_controler_interface,
            &unexpected_blocks.iter().copied().collect(),
            2000
        )
        .await
    );
}

#[tokio::test]
async fn test_add_block_that_depends_on_invalid_block() {
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/
    let node_ids = tools::create_node_ids(1);

    let (protocol_controller, mut protocol_controler_interface) = mock_protocol_controller::new();
    let mut cfg = tools::default_consensus_config(&node_ids);
    cfg.t0 = 1000.into();
    cfg.genesis_timestamp = UTime::now()
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(1000).unwrap());

    cfg.max_dependency_blocks = 7;

    let cnss = DefaultConsensusController::new(&cfg, protocol_controller)
        .await
        .expect("Could not create consensus controller");

    let genesis_hashes = cnss
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    //create test blocks
    let (hasht0s1, t0s1, _) = tools::create_block(&cfg, 0, 1, genesis_hashes.clone());

    let (hasht1s1, t1s1, _) = tools::create_block(&cfg, 1, 1, genesis_hashes.clone());

    // blocks t3s2 with wrong thread and (t0s1, t1s1) parents.
    let (hasht3s2, t3s2, _) = tools::create_block(&cfg, 3, 2, vec![hasht0s1, hasht1s1]);

    // blocks t0s3 and t1s3 with (t3s2, t1s2) parents.
    let (hasht0s3, t0s3, _) = tools::create_block(&cfg, 0, 3, vec![hasht3s2, hasht1s1]);
    let (hasht1s3, t1s3, _) = tools::create_block(&cfg, 1, 3, vec![hasht3s2, hasht1s1]);

    // add block in this order t0s1, t1s1, t0s3, t1s3, t3s2
    //send blocks   t0s2, t1s3, t0s1, t0s4, t1s4, t1s1, t0s3, t1s2
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t0s1)
        .await;
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t1s1)
        .await;
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t0s3)
        .await;
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t1s3)
        .await;
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &t3s2)
        .await;

    //block t0s1 and t1s1 are propagated
    let hash_list = vec![hasht0s1, hasht1s1];
    tools::validate_propagate_block_in_list(&mut protocol_controler_interface, &hash_list, 1000)
        .await;
    tools::validate_propagate_block_in_list(&mut protocol_controler_interface, &hash_list, 1000)
        .await;

    //block  t0s3, t1s3 are not propagated
    let hash_list = vec![hasht0s3, hasht1s3];
    assert!(
        !tools::validate_notpropagate_block_in_list(
            &mut protocol_controler_interface,
            &hash_list,
            2000
        )
        .await
    );
    assert!(
        !tools::validate_notpropagate_block_in_list(
            &mut protocol_controler_interface,
            &hash_list,
            2000
        )
        .await
    );
}
