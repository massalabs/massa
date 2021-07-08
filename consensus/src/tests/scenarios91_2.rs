use super::{mock_protocol_controller::MockProtocolController, tools};
use crate::{
    random_selector::RandomSelector, start_consensus_controller,
    timeslots::get_block_slot_timestamp,
};
use communication::protocol::ProtocolCommand;
use crypto::hash::Hash;
use time::UTime;
use tokio::time::{sleep_until, timeout};

#[tokio::test]
async fn test_queueing() {
    // setup logging
    // stderrlog::new()
    //     .verbosity(3)
    //     .timestamp(stderrlog::Timestamp::Millisecond)
    //     .init()
    //     .unwrap();

    let node_ids = tools::create_node_ids(1);

    let mut cfg = tools::default_consensus_config(&node_ids);
    cfg.t0 = 32000.into();
    cfg.delta_f0 = 32;

    //to avoid timing problems for block in the future
    cfg.genesis_timestamp = UTime::now()
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(1000).unwrap());

    // mock protocol
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();

    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
            None,
        )
        .await
        .expect("could not start consensus controller");

    let genesis_hashes = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    // * create 30 normal blocks in each thread: in slot 1 they have genesis parents, in slot 2 they have slot 1 parents
    //create a valid block for slot 1
    let mut valid_hasht0 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        node_ids[0].1.clone(),
        0,
        1,
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    //create a valid block on the other thread.
    let mut valid_hasht1 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        node_ids[0].1.clone(),
        1,
        1,
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    // and loop for the 29 other blocks
    for i in 0..29 {
        valid_hasht0 = tools::create_and_test_block(
            &mut protocol_controller,
            &cfg,
            node_ids[0].1.clone(),
            0,
            i + 2,
            vec![valid_hasht0, valid_hasht1],
            true,
            false,
        )
        .await;

        //create a valid block on the other thread.
        valid_hasht1 = tools::create_and_test_block(
            &mut protocol_controller,
            &cfg,
            node_ids[0].1.clone(),
            1,
            i + 2,
            vec![valid_hasht0, valid_hasht1],
            true,
            false,
        )
        .await;
    }

    let (missed_hash, _missed_block, _missed_key) =
        tools::create_block(&cfg, 0, 32, vec![valid_hasht0, valid_hasht1]);

    //create 1 block in thread 0 slot 33 with missed block as parent
    valid_hasht0 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        node_ids[0].1.clone(),
        0,
        33,
        vec![missed_hash, valid_hasht1],
        false,
        false,
    )
    .await;

    // and loop again for the 99 other blocks
    for i in 0..30 {
        valid_hasht0 = tools::create_and_test_block(
            &mut protocol_controller,
            &cfg,
            node_ids[0].1.clone(),
            0,
            i + 34,
            vec![valid_hasht0, valid_hasht1],
            false,
            false,
        )
        .await;

        //create a valid block on the other thread.
        valid_hasht1 = tools::create_and_test_block(
            &mut protocol_controller,
            &cfg,
            node_ids[0].1.clone(),
            1,
            i + 34,
            vec![valid_hasht0, valid_hasht1],
            false,
            false,
        )
        .await;
    }

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_doubles() {
    // setup logging
    // stderrlog::new()
    //     .verbosity(3)
    //     .timestamp(stderrlog::Timestamp::Millisecond)
    //     .init()
    //     .unwrap();

    let node_ids = tools::create_node_ids(1);

    let mut cfg = tools::default_consensus_config(&node_ids);
    cfg.t0 = 32000.into();
    cfg.delta_f0 = 32;

    //to avoid timing pb for block in the future
    cfg.genesis_timestamp = UTime::now()
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(1000).unwrap());

    // mock protocol
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();

    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
            None,
        )
        .await
        .expect("could not start consensus controller");

    let genesis_hashes = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    // * create 40 normal blocks in each thread: in slot 1 they have genesis parents, in slot 2 they have slot 1 parents
    //create a valid block for slot 1
    let mut valid_hasht0 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        node_ids[0].1.clone(),
        0,
        1,
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    //create a valid block on the other thread.
    let mut valid_hasht1 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        node_ids[0].1.clone(),
        1,
        1,
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    // and loop for the 39 other blocks
    for i in 0..39 {
        valid_hasht0 = tools::create_and_test_block(
            &mut protocol_controller,
            &cfg,
            node_ids[0].1.clone(),
            0,
            i + 2,
            vec![valid_hasht0, valid_hasht1],
            true,
            false,
        )
        .await;

        //create a valid block on the other thread.
        valid_hasht1 = tools::create_and_test_block(
            &mut protocol_controller,
            &cfg,
            node_ids[0].1.clone(),
            1,
            i + 2,
            vec![valid_hasht0, valid_hasht1],
            true,
            false,
        )
        .await;
    }

    //create 1 block in thread 0 slot 41 with missed block as parent
    valid_hasht0 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        node_ids[0].1.clone(),
        0,
        41,
        vec![valid_hasht0, valid_hasht1],
        true,
        false,
    )
    .await;

    if let Some(block) = consensus_command_sender
        .get_active_block(valid_hasht0)
        .await
        .unwrap()
    {
        tools::propagate_block(
            &mut protocol_controller,
            node_ids[0].1.clone(),
            block,
            false,
        )
        .await;
    };

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_double_staking() {
    // setup logging
    // stderrlog::new()
    //     .verbosity(3)
    //     .timestamp(stderrlog::Timestamp::Millisecond)
    //     .init()
    //     .unwrap();

    let node_ids = tools::create_node_ids(1);

    let mut cfg = tools::default_consensus_config(&node_ids);
    cfg.t0 = 32000.into();
    cfg.delta_f0 = 32;

    //to avoid timing pb for block in the future
    cfg.genesis_timestamp = UTime::now()
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(1000).unwrap());

    // mock protocol
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();

    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
            None,
        )
        .await
        .expect("could not start consensus controller");

    let genesis_hashes = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    // * create 40 normal blocks in each thread: in slot 1 they have genesis parents, in slot 2 they have slot 1 parents
    //create a valid block for slot 1
    let mut valid_hasht0 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        node_ids[0].1.clone(),
        0,
        1,
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    //create a valid block on the other thread.
    let mut valid_hasht1 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        node_ids[0].1.clone(),
        1,
        1,
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    // and loop for the 39 other blocks
    for i in 0..39 {
        valid_hasht0 = tools::create_and_test_block(
            &mut protocol_controller,
            &cfg,
            node_ids[0].1.clone(),
            0,
            i + 2,
            vec![valid_hasht0, valid_hasht1],
            true,
            false,
        )
        .await;

        //create a valid block on the other thread.
        valid_hasht1 = tools::create_and_test_block(
            &mut protocol_controller,
            &cfg,
            node_ids[0].1.clone(),
            1,
            i + 2,
            vec![valid_hasht0, valid_hasht1],
            true,
            false,
        )
        .await;
    }

    // same creator same slot, different block
    let operation_merkle_root = Hash::hash("42".as_bytes());
    let (hash_1, block_1, _key) = tools::create_block_with_merkle_root(
        &cfg,
        operation_merkle_root,
        0,
        41,
        vec![valid_hasht0, valid_hasht1],
    );
    tools::propagate_block(
        &mut protocol_controller,
        node_ids[0].1.clone(),
        block_1,
        true,
    )
    .await;

    let operation_merkle_root = Hash::hash("so long and thanks for all the fish".as_bytes());
    let (hash_2, block_2, _key) = tools::create_block_with_merkle_root(
        &cfg,
        operation_merkle_root,
        0,
        41,
        vec![valid_hasht0, valid_hasht1],
    );
    tools::propagate_block(
        &mut protocol_controller,
        node_ids[0].1.clone(),
        block_2,
        true,
    )
    .await;

    let graph = consensus_command_sender
        .get_block_graph_status()
        .await
        .unwrap();
    let cliques_1 = tools::get_cliques(&graph, hash_1);
    let cliques_2 = tools::get_cliques(&graph, hash_2);
    assert!(cliques_1.is_disjoint(&cliques_2));

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_test_parents() {
    // // setup logging
    // stderrlog::new()
    //     .verbosity(4)
    //     .timestamp(stderrlog::Timestamp::Millisecond)
    //     .init()
    //     .unwrap();

    let node_ids = tools::create_node_ids(1);

    let mut cfg = tools::default_consensus_config(&node_ids);
    cfg.t0 = 32000.into();
    cfg.delta_f0 = 32;

    //to avoid timing pb for block in the future
    cfg.genesis_timestamp = UTime::now()
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(1000).unwrap());

    // mock protocol
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();

    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
            None,
        )
        .await
        .expect("could not start consensus controller");

    let genesis_hashes = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    // * create 2 normal blocks in each thread: in slot 1 they have genesis parents, in slot 2 they have slot 1 parents
    //create a valid block for slot 1
    let valid_hasht0s1 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        node_ids[0].1.clone(),
        0,
        1,
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    //create a valid block on the other thread.
    let valid_hasht1s1 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        node_ids[0].1.clone(),
        1,
        1,
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    //create a valids block for slot 2
    let valid_hasht0s2 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        node_ids[0].1.clone(),
        0,
        2,
        vec![valid_hasht0s1, valid_hasht1s1],
        true,
        false,
    )
    .await;

    //create a valid block on the other thread.
    tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        node_ids[0].1.clone(),
        1,
        2,
        vec![valid_hasht0s1, valid_hasht1s1],
        true,
        false,
    )
    .await;

    // * create 1 block in t0s3 with parents (t0s2, t1s0)
    //create a valids block for slot 2
    tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        node_ids[0].1.clone(),
        0,
        3,
        vec![valid_hasht0s2, genesis_hashes[1]],
        false,
        false,
    )
    .await;

    // * create 1 block in t1s3 with parents (t0s0, t0s0)
    tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        node_ids[0].1.clone(),
        1,
        3,
        vec![genesis_hashes[0], genesis_hashes[0]],
        false,
        false,
    )
    .await;

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_block_creation() {
    let node_ids = tools::create_node_ids(2);

    let mut cfg = tools::default_consensus_config(&node_ids);
    cfg.t0 = 1000.into();
    cfg.delta_f0 = 32;
    cfg.disable_block_creation = false;
    cfg.thread_count = 1;

    let seed = vec![0u8; 32]; // TODO temporary (see issue #103)
    let participants_weights = vec![1u64; cfg.nodes.len()]; // TODO (see issue #104)
    let mut selector = RandomSelector::new(&seed, cfg.thread_count, participants_weights).unwrap();
    let mut expected_slots = Vec::new();
    for i in 1..11 {
        expected_slots.push(selector.draw((i, 0)))
    }

    //to avoid timing pb for block in the future
    cfg.genesis_timestamp = UTime::now().unwrap();

    // mock protocol
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();

    // launch consensus controller
    let (_consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
            None,
        )
        .await
        .expect("could not start consensus controller");

    for (i, &draw) in expected_slots.iter().enumerate() {
        sleep_until(
            get_block_slot_timestamp(
                cfg.thread_count,
                cfg.t0,
                cfg.genesis_timestamp,
                ((i + 1usize) as u64, 0),
            )
            .unwrap()
            .estimate_instant()
            .unwrap(),
        )
        .await;

        match timeout(
            cfg.t0.checked_div_u64(2).unwrap().into(),
            protocol_controller.wait_command(),
        )
        .await
        {
            Ok(Some(ProtocolCommand::PropagateBlock { hash: _, block })) => {
                assert_eq!(draw, 0);
                assert_eq!(i + 1, block.header.period_number as usize);
            }
            // Ok(Some(_)) => panic!("unexpected command"),
            Ok(None) => panic!("an error occurs while waiting for ProtocolCommand event"),
            Err(_) => assert_eq!(draw, 1),
        };
    }

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}
