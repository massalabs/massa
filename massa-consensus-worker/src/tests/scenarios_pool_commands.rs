// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::tools::*;
use massa_consensus_exports::ConsensusConfig;

use massa_graph::{ledger::LedgerSubset, BootstrapableGraph};
use massa_models::clique::Clique;
use massa_models::ledger_models::LedgerData;
use massa_models::signed::{Signable, Signed};
use massa_models::{Amount, BlockId, Operation, OperationId, Slot};
use massa_pool::PoolCommand;
use massa_signature::{generate_random_private_key, PrivateKey, PublicKey};
use massa_time::MassaTime;
use serial_test::serial;
use std::str::FromStr;

#[tokio::test]
#[serial]
async fn test_update_current_slot_cmd_notification() {
    let cfg = ConsensusConfig {
        t0: 1000.into(),
        thread_count: 1,
        genesis_timestamp: MassaTime::now().unwrap().checked_add(1000.into()).unwrap(),
        ..ConsensusConfig::default_with_paths()
    };

    let timeout = 150;

    consensus_pool_test(
        cfg.clone(),
        None,
        None,
        async move |mut pool_controller,
                    protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver| {
            let slot_notification_filter = |cmd| match cmd {
                massa_pool::PoolCommand::UpdateCurrentSlot(slot) => {
                    Some((slot, MassaTime::now().unwrap()))
                }
                _ => None,
            };

            // wait for UpdateCurrentSlot pool command
            if let Some((slot_cmd, rec_time)) = pool_controller
                .wait_command(1500.into(), slot_notification_filter)
                .await
            {
                assert_eq!(slot_cmd, Slot::new(0, 0));
                if rec_time > cfg.genesis_timestamp {
                    assert!(
                        rec_time.saturating_sub(cfg.genesis_timestamp) < MassaTime::from(timeout)
                    )
                } else {
                    assert!(
                        cfg.genesis_timestamp.saturating_sub(rec_time) < MassaTime::from(timeout)
                    )
                }
            }

            // wait for next UpdateCurrentSlot pool command
            if let Some((slot_cmd, rec_time)) = pool_controller
                .wait_command(500.into(), slot_notification_filter)
                .await
            {
                assert_eq!(slot_cmd, Slot::new(0, 1));
                if rec_time > cfg.genesis_timestamp {
                    assert!(
                        rec_time.saturating_sub(cfg.genesis_timestamp.saturating_add(cfg.t0))
                            < MassaTime::from(timeout)
                    );
                } else {
                    assert!(
                        cfg.genesis_timestamp
                            .saturating_add(cfg.t0)
                            .saturating_sub(rec_time)
                            < MassaTime::from(timeout)
                    );
                }
            }
            (
                pool_controller,
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_update_latest_final_block_cmd_notification() {
    let cfg = ConsensusConfig {
        t0: 1000.into(),
        delta_f0: 2,
        disable_block_creation: false,
        ..ConsensusConfig::default_with_paths()
    };

    consensus_pool_test(
        cfg.clone(),
        None,
        None,
        async move |mut pool_controller,
                    protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver| {
            // UpdateLatestFinalPeriods pool command filter
            let update_final_notification_filter = |cmd| match cmd {
                massa_pool::PoolCommand::UpdateLatestFinalPeriods(periods) => Some(periods),
                PoolCommand::GetOperationBatch { response_tx, .. } => {
                    response_tx.send(Vec::new()).unwrap();
                    None
                }
                PoolCommand::GetEndorsements { response_tx, .. } => {
                    response_tx.send(Vec::new()).unwrap();
                    None
                }
                _ => None,
            };
            // wait for initial final periods notification
            let final_periods = pool_controller
                .wait_command(1000.into(), update_final_notification_filter)
                .await;
            assert_eq!(final_periods, Some(vec![0, 0]));

            // wait for next final periods notification
            let final_periods = pool_controller
                .wait_command(
                    (cfg.t0.to_millis() * 3).into(),
                    update_final_notification_filter,
                )
                .await;
            assert_eq!(final_periods, Some(vec![1, 0]));
            (
                pool_controller,
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_new_final_ops() {
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let cfg = ConsensusConfig {
        t0: 1000.into(),
        delta_f0: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        ..ConsensusConfig::default_with_staking_keys(&staking_keys)
    };

    // define addresses use for the test
    // addresses a and b both in thread 0

    let (address_a, priv_a, pubkey_a) = random_address_on_thread(0, cfg.thread_count).into();
    let (address_b, _, _) = random_address_on_thread(0, cfg.thread_count).into();

    let boot_ledger = LedgerSubset(
        vec![(address_a, LedgerData::new(Amount::from_str("100").unwrap()))]
            .into_iter()
            .collect(),
    );
    let op = create_transaction(priv_a, pubkey_a, address_b, 1, 10, 1);
    let (boot_graph, mut p0, mut p1) = get_bootgraph(pubkey_a, op.clone(), boot_ledger);

    consensus_pool_test(
        cfg.clone(),
        None,
        Some(boot_graph),
        async move |mut pool_controller,
                    mut protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver| {
            p1 = create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(1, 1),
                vec![p0, p1],
                true,
                false,
                staking_keys[0],
            )
            .await;

            p0 = create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(2, 0),
                vec![p0, p1],
                true,
                false,
                staking_keys[0],
            )
            .await;

            create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(2, 1),
                vec![p0, p1],
                true,
                false,
                staking_keys[0],
            )
            .await;
            // UpdateLatestFinalPeriods pool command filter
            let new_final_ops_filter = |cmd| match cmd {
                PoolCommand::FinalOperations(ops) => Some(ops),
                _ => None,
            };

            // wait for initial final periods notification
            let final_ops = pool_controller
                .wait_command(300.into(), new_final_ops_filter)
                .await;
            if let Some(finals) = final_ops {
                assert!(finals.contains_key(&op.content.compute_id().unwrap()));
                assert_eq!(
                    finals.get(&op.content.compute_id().unwrap()),
                    Some(&(10, 0))
                )
            } else {
                panic!("no final ops")
            }
            (
                pool_controller,
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_max_attempts_get_operations() {
    let cfg = ConsensusConfig {
        t0: 1000.into(),
        genesis_timestamp: MassaTime::now().unwrap().checked_sub(1000.into()).unwrap(),
        delta_f0: 2,
        disable_block_creation: false,
        operation_batch_size: 1,
        ..ConsensusConfig::default_with_paths()
    };
    // define addresses use for the test
    // addresses a and b both in thread 0
    let (address_a, priv_a, pubkey_a) = random_address_on_thread(0, cfg.thread_count).into();
    let (address_b, _, _) = random_address_on_thread(0, cfg.thread_count).into();

    let boot_ledger = LedgerSubset(
        vec![(address_a, LedgerData::new(Amount::from_str("100").unwrap()))]
            .into_iter()
            .collect(),
    );
    let op = create_transaction(priv_a, pubkey_a, address_b, 1, 10, 1);
    let (boot_graph, _p0, _p1) = get_bootgraph(pubkey_a, op.clone(), boot_ledger);

    consensus_pool_test(
        cfg.clone(),
        None,
        Some(boot_graph),
        async move |mut pool_controller,
                    protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver| {
            // Test that consensus keeps trying to fill the block,
            // until the max number of attempts has been reached.
            let mut attempts = 0;
            let mut slot = None;
            while attempts != cfg.max_operations_fill_attempts {
                let get_operations_batch_filter = |cmd| match cmd {
                    PoolCommand::GetOperationBatch {
                        response_tx,
                        target_slot,
                        ..
                    } => Some((response_tx, target_slot)),
                    PoolCommand::GetEndorsements { response_tx, .. } => {
                        response_tx.send(Vec::new()).unwrap();
                        None
                    }
                    _ => None,
                };

                let (response_tx, target_slot) = pool_controller
                    .wait_command(1000.into(), get_operations_batch_filter)
                    .await
                    .expect("No response chan and target slot.");

                // Test that the batch requests are still for the same slot.
                if let Some(slot) = slot {
                    assert_eq!(slot, target_slot);
                } else {
                    slot = Some(target_slot);
                }

                // Send a full batch back.
                response_tx
                    .send(vec![(
                        op.clone().content.compute_id().unwrap(),
                        op.clone(),
                        10,
                    )])
                    .expect("Failed to send empty batch.");
                attempts += 1;
            }

            // The next command should be a slot update.
            let slot_filter = |cmd| match cmd {
                PoolCommand::UpdateCurrentSlot(slot) => Some(slot),
                PoolCommand::GetEndorsements { response_tx, .. } => {
                    response_tx.send(Vec::new()).unwrap();
                    None
                }
                _ => None,
            };

            pool_controller.wait_command(3000.into(), slot_filter).await;
            (
                pool_controller,
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_max_batch_size_get_operations() {
    let cfg = ConsensusConfig {
        t0: 1000.into(),
        genesis_timestamp: MassaTime::now().unwrap().checked_sub(1000.into()).unwrap(),
        delta_f0: 2,
        disable_block_creation: false,
        operation_batch_size: 10,
        ..ConsensusConfig::default_with_paths()
    };
    // define addresses use for the test
    // addresses a and b both in thread 0
    let (address_a, priv_a, pubkey_a) = random_address_on_thread(0, cfg.thread_count).into();
    let (address_b, _, _) = random_address_on_thread(0, cfg.thread_count).into();

    let boot_ledger = LedgerSubset(
        vec![(address_a, LedgerData::new(Amount::from_str("100").unwrap()))]
            .into_iter()
            .collect(),
    );
    let op = create_transaction(priv_a, pubkey_a, address_b, 1, 10, 1);
    let (boot_graph, _p0, _p1) = get_bootgraph(pubkey_a, op.clone(), boot_ledger);

    consensus_pool_test(
        cfg.clone(),
        None,
        Some(boot_graph),
        async move |mut pool_controller,
                    protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver| {
            // Test that consensus stops trying to fill the block,
            // once a non-full batch has been received.
            let get_operations_batch_filter = |cmd| match cmd {
                PoolCommand::GetOperationBatch {
                    response_tx,
                    target_slot,
                    ..
                } => Some((response_tx, target_slot)),
                PoolCommand::GetEndorsements { response_tx, .. } => {
                    response_tx.send(Vec::new()).unwrap();
                    None
                }
                _ => None,
            };

            let (response_tx, target_slot) = pool_controller
                .wait_command(1000.into(), get_operations_batch_filter)
                .await
                .expect("No response chan and target slot.");

            // Send a non-full batch back.
            response_tx
                .send(vec![(
                    op.clone().content.compute_id().unwrap(),
                    op.clone(),
                    10,
                )])
                .expect("Failed to send non-full batch.");

            // The next command should be a slot update.
            let slot_filter = |cmd| match cmd {
                PoolCommand::UpdateCurrentSlot(slot) => Some(slot),
                _ => None,
            };

            let slot_update = pool_controller
                .wait_command(3000.into(), slot_filter)
                .await
                .expect("Not slot update received.");

            // Test that the update is for the slot
            // after the one for the just created block.
            assert_eq!(slot_update.period, target_slot.period + 1);
            (
                pool_controller,
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}

fn get_bootgraph(
    creator: PublicKey,
    operation: Signed<Operation, OperationId>,
    ledger: LedgerSubset,
) -> (BootstrapableGraph, BlockId, BlockId) {
    let (genesis_0, g0_id) =
        get_export_active_test_block(creator, vec![], vec![], Slot::new(0, 0), true);
    let (genesis_1, g1_id) =
        get_export_active_test_block(creator, vec![], vec![], Slot::new(0, 1), true);
    let (p1t0, p1t0_id) = get_export_active_test_block(
        creator,
        vec![(g0_id, 0), (g1_id, 0)],
        vec![operation],
        Slot::new(1, 0),
        false,
    );
    (
        BootstrapableGraph {
            /// Map of active blocks, where blocks are in their exported version.
            active_blocks: vec![(g0_id, genesis_0), (g1_id, genesis_1), (p1t0_id, p1t0)]
                .into_iter()
                .collect(),
            /// Best parents hash in each thread.
            best_parents: vec![(p1t0_id, 1), (g1_id, 0)],
            /// Latest final period and block hash in each thread.
            latest_final_blocks_periods: vec![(g0_id, 0u64), (g1_id, 0u64)],
            /// Head of the incompatibility graph.
            gi_head: vec![
                (g0_id, Default::default()),
                (p1t0_id, Default::default()),
                (g1_id, Default::default()),
            ]
            .into_iter()
            .collect(),

            /// List of maximal cliques of compatible blocks.
            max_cliques: vec![Clique {
                block_ids: vec![g0_id, p1t0_id, g1_id].into_iter().collect(),
                fitness: 1111,
                is_blockclique: true,
            }],
            ledger,
        },
        p1t0_id,
        g1_id,
    )
}
