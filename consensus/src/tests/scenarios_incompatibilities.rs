use super::{
    mock_pool_controller::{MockPoolController, PoolCommandSink},
    mock_protocol_controller::MockProtocolController,
    tools,
};
use crate::{start_consensus_controller, tests::tools::generate_ledger_file};
use models::Slot;
use serial_test::serial;
use std::collections::{HashMap, VecDeque};

#[tokio::test]
#[serial]
async fn test_thread_incompatibility() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let mut cfg = tools::default_consensus_config(1, ledger_file.path());
    cfg.t0 = 200.into();
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

    let parents = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .best_parents;

    let hash_1 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        Slot::new(1, 0),
        parents.clone(),
        true,
        false,
    )
    .await;

    let hash_2 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        Slot::new(1, 1),
        parents.clone(),
        true,
        false,
    )
    .await;

    let hash_3 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        Slot::new(2, 0),
        parents.clone(),
        true,
        false,
    )
    .await;

    let status = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status");

    if hash_1 > hash_3 {
        assert_eq!(status.best_parents[0], hash_3);
    } else {
        assert_eq!(status.best_parents[0], hash_1);
    }
    assert_eq!(status.best_parents[1], hash_2);

    assert!(if let Some(h) = status.gi_head.get(&hash_3) {
        h.contains(&hash_1)
    } else {
        panic!("missign hash in gi_head")
    });

    assert_eq!(status.max_cliques.len(), 2);

    for clique in status.max_cliques.clone() {
        if clique.contains(&hash_1) && clique.contains(&hash_3) {
            panic!("incompatible bloocks in the same clique")
        }
    }

    let mut current_period = 3;
    let mut parents = vec![hash_1, hash_2];
    for _ in 0..3 as usize {
        let hash = tools::create_and_test_block(
            &mut protocol_controller,
            &cfg,
            Slot::new(current_period, 0),
            parents.clone(),
            true,
            false,
        )
        .await;
        current_period += 1;
        parents[0] = hash.clone();
    }

    let status = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status");

    assert!(if let Some(h) = status.gi_head.get(&hash_3) {
        h.contains(&status.best_parents[0])
    } else {
        panic!("missing block in clique")
    });

    let mut parents = vec![status.best_parents[0].clone(), hash_2];
    let mut current_period = 8;
    for _ in 0..30 as usize {
        let (hash, b, _) = tools::create_block(
            &cfg,
            Slot::new(current_period, 0),
            parents.clone(),
            cfg.nodes[0].clone(),
        );
        current_period += 1;
        parents[0] = hash.clone();
        protocol_controller.receive_block(b).await;

        // Note: higher timeout required.
        tools::validate_propagate_block_in_list(&mut protocol_controller, &vec![hash], 5000).await;
    }

    let status = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status");

    assert_eq!(status.max_cliques.len(), 1);

    // clique should have been deleted by now
    let parents = vec![hash_3, hash_2];
    let _ = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        Slot::new(40, 0),
        parents.clone(),
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
    pool_sink.stop().await;
}

#[tokio::test]
#[serial]
async fn test_grandpa_incompatibility() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let mut cfg = tools::default_consensus_config(1, ledger_file.path());
    cfg.t0 = 200.into();
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

    let genesis = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    let hash_1 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        Slot::new(1, 0),
        vec![genesis[0], genesis[1]],
        true,
        false,
    )
    .await;

    let hash_2 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        Slot::new(1, 1),
        vec![genesis[0], genesis[1]],
        true,
        false,
    )
    .await;

    let hash_3 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        Slot::new(2, 0),
        vec![hash_1, genesis[1]],
        true,
        false,
    )
    .await;

    let hash_4 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        Slot::new(2, 1),
        vec![genesis[0], hash_2],
        true,
        false,
    )
    .await;

    let status = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status");

    assert!(if let Some(h) = status.gi_head.get(&hash_4) {
        h.contains(&hash_3)
    } else {
        panic!("missign block in gi_head")
    });

    assert_eq!(status.max_cliques.len(), 2);

    for clique in status.max_cliques.clone() {
        if clique.contains(&hash_3) && clique.contains(&hash_4) {
            panic!("incompatible blocks in the same clique")
        }
    }

    let parents = status.best_parents.clone();
    if hash_4 > hash_3 {
        assert_eq!(parents[0], hash_3)
    } else {
        assert_eq!(parents[1], hash_4)
    }

    let mut latest_extra_blocks = VecDeque::new();
    for extend_i in 0..33 {
        let status = consensus_command_sender
            .get_block_graph_status()
            .await
            .expect("could not get block graph status");
        let hash = tools::create_and_test_block(
            &mut protocol_controller,
            &cfg,
            Slot::new(3 + extend_i, 0),
            status.best_parents,
            true,
            false,
        )
        .await;

        latest_extra_blocks.push_back(hash);
        while latest_extra_blocks.len() > cfg.delta_f0 as usize + 1 {
            latest_extra_blocks.pop_front();
        }
    }

    let latest_extra_blocks = latest_extra_blocks.into_iter().collect();
    let status = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status");
    assert_eq!(
        status.max_cliques,
        vec![latest_extra_blocks],
        "wrong cliques"
    );

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
    pool_sink.stop().await;
}
