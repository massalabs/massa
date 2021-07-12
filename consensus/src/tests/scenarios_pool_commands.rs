// Copyright (c) 2021 MASSA LABS <info@massa.net>

use std::collections::HashMap;

use crate::{
    tests::tools::{self, create_transaction, generate_ledger_file, get_export_active_test_block},
    BootsrapableGraph, LedgerData, LedgerExport,
};
use crypto::signature::PublicKey;
use models::{Address, BlockId, Operation, Slot};
use pool::PoolCommand;
use serial_test::serial;
use time::UTime;

#[tokio::test]
#[serial]
async fn test_update_current_slot_cmd_notification() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<crypto::signature::PrivateKey> = (0..1)
        .map(|_| crypto::generate_random_private_key())
        .collect();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.t0 = 2000.into();
    cfg.genesis_timestamp = UTime::now(0).unwrap().checked_sub(100.into()).unwrap();

    tools::consensus_pool_test(
        cfg.clone(),
        None,
        None,
        None,
        async move |mut pool_controller,
                    protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver| {
            let slot_notification_filter = |cmd| match cmd {
                pool::PoolCommand::UpdateCurrentSlot(slot) => Some(slot),
                _ => None,
            };

            //wait for UpdateCurrentSlot pool command
            let slot_cmd = pool_controller
                .wait_command(500.into(), slot_notification_filter)
                .await;
            assert_eq!(slot_cmd, Some(Slot::new(0, 0)));

            //wait for next UpdateCurrentSlot pool command
            let slot_cmd = pool_controller
                .wait_command(2000.into(), slot_notification_filter)
                .await;
            assert_eq!(slot_cmd, Some(Slot::new(0, 1)));
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
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<crypto::signature::PrivateKey> = (0..1)
        .map(|_| crypto::generate_random_private_key())
        .collect();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.t0 = 1000.into();
    cfg.genesis_timestamp = UTime::now(0).unwrap().checked_sub(100.into()).unwrap();
    cfg.delta_f0 = 2;
    cfg.disable_block_creation = false;

    tools::consensus_pool_test(
        cfg.clone(),
        None,
        None,
        None,
        async move |mut pool_controller,
                    protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver| {
            // UpdateLatestFinalPeriods pool command filter
            let update_final_notification_filter = |cmd| match cmd {
                pool::PoolCommand::UpdateLatestFinalPeriods(periods) => Some(periods),
                PoolCommand::GetOperationBatch { response_tx, .. } => {
                    response_tx.send(Vec::new()).unwrap();
                    None
                }
                _ => None,
            };

            // wait for initial final periods notification
            let final_periods = pool_controller
                .wait_command(300.into(), update_final_notification_filter)
                .await;
            assert_eq!(final_periods, Some(vec![0, 0]));

            // wait for next final periods notification
            let final_periods = pool_controller
                .wait_command(
                    (cfg.t0.to_millis() * 2 + 500).into(),
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
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<crypto::signature::PrivateKey> = (0..1)
        .map(|_| crypto::generate_random_private_key())
        .collect();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.t0 = 1000.into();
    cfg.genesis_timestamp = UTime::now(0).unwrap().checked_sub(cfg.t0).unwrap();
    cfg.delta_f0 = 2;
    cfg.disable_block_creation = true;

    let thread_count = 2;
    //define addresses use for the test
    // addresses a and b both in thread 0
    let mut priv_a = crypto::generate_random_private_key();
    let mut pubkey_a = crypto::derive_public_key(&priv_a);
    let mut address_a = Address::from_public_key(&pubkey_a).unwrap();
    while 0 != address_a.get_thread(thread_count) {
        priv_a = crypto::generate_random_private_key();
        pubkey_a = crypto::derive_public_key(&priv_a);
        address_a = Address::from_public_key(&pubkey_a).unwrap();
    }
    assert_eq!(0, address_a.get_thread(thread_count));

    let mut priv_b = crypto::generate_random_private_key();
    let mut pubkey_b = crypto::derive_public_key(&priv_b);
    let mut address_b = Address::from_public_key(&pubkey_b).unwrap();
    while 0 != address_b.get_thread(thread_count) {
        priv_b = crypto::generate_random_private_key();
        pubkey_b = crypto::derive_public_key(&priv_b);
        address_b = Address::from_public_key(&pubkey_b).unwrap();
    }
    assert_eq!(0, address_b.get_thread(thread_count));

    let boot_ledger = LedgerExport {
        ledger_per_thread: vec![vec![(address_a, LedgerData { balance: 100 })], vec![]],
    };
    let op = create_transaction(priv_a, pubkey_a, address_b, 1, 10, 1);
    let (boot_graph, mut p0, mut p1) = get_bootgraph(pubkey_a, op.clone(), boot_ledger);

    tools::consensus_pool_test(
        cfg.clone(),
        None,
        None,
        Some(boot_graph),
        async move |mut pool_controller,
                    mut protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver| {
            p1 = tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(1, 1),
                vec![p0, p1],
                true,
                false,
                staking_keys[0].clone(),
            )
            .await;

            p0 = tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(2, 0),
                vec![p0, p1],
                true,
                false,
                staking_keys[0].clone(),
            )
            .await;

            tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(2, 1),
                vec![p0, p1],
                true,
                false,
                staking_keys[0].clone(),
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
                assert!(finals.contains_key(&op.get_operation_id().unwrap()));
                assert_eq!(finals.get(&op.get_operation_id().unwrap()), Some(&(10, 0)))
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

fn get_bootgraph(
    creator: PublicKey,
    operation: Operation,
    ledger: LedgerExport,
) -> (BootsrapableGraph, BlockId, BlockId) {
    let (genesis_0, g0_id) =
        get_export_active_test_block(creator.clone(), vec![], vec![], Slot::new(0, 0), true);
    let (genesis_1, g1_id) =
        get_export_active_test_block(creator.clone(), vec![], vec![], Slot::new(0, 1), true);
    let (p1t0, p1t0_id) = get_export_active_test_block(
        creator.clone(),
        vec![(g0_id, 0), (g1_id, 0)],
        vec![operation],
        Slot::new(1, 0),
        false,
    );
    (
        BootsrapableGraph {
            /// Map of active blocks, where blocks are in their exported version.
            active_blocks: vec![
                (g0_id, genesis_0.clone()),
                (g1_id, genesis_1.clone()),
                (p1t0_id, p1t0.clone()),
            ]
            .into_iter()
            .collect(),
            /// Best parents hash in each thread.
            best_parents: vec![p1t0_id, g1_id],
            /// Latest final period and block hash in each thread.
            latest_final_blocks_periods: vec![(g0_id, 0u64), (g1_id, 0u64)],
            /// Head of the incompatibility graph.
            gi_head: vec![(g0_id, vec![]), (p1t0_id, vec![]), (g1_id, vec![])]
                .into_iter()
                .collect(),

            /// List of maximal cliques of compatible blocks.
            max_cliques: vec![vec![g0_id, p1t0_id, g1_id]],
            ledger,
        },
        p1t0_id,
        g1_id,
    )
}
