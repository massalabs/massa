// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::{
    tests::tools::{self, create_transaction, generate_ledger_file, get_export_active_test_block},
    BootstrapableGraph, LedgerSubset,
};
use models::clique::Clique;
use models::ledger::LedgerData;
use models::{
    Address, Amount, BlockId, Operation, OperationSearchResult, OperationSearchResultStatus, Slot,
};
use serial_test::serial;
use signature::{derive_public_key, generate_random_private_key, PrivateKey, PublicKey};
use std::collections::HashMap;
use std::str::FromStr;
use time::UTime;

#[tokio::test]
#[serial]
async fn test_get_operation() {
    // // setup logging
    // stderrlog::new()
    //     .verbosity(4)
    //     .timestamp(stderrlog::Timestamp::Millisecond)
    //     .init()
    //     .unwrap();
    let thread_count = 2;
    // define addresses use for the test
    // addresses a and b both in thread 0
    let mut priv_a = generate_random_private_key();
    let mut pubkey_a = derive_public_key(&priv_a);
    let mut address_a = Address::from_public_key(&pubkey_a).unwrap();
    while 0 != address_a.get_thread(thread_count) {
        priv_a = generate_random_private_key();
        pubkey_a = derive_public_key(&priv_a);
        address_a = Address::from_public_key(&pubkey_a).unwrap();
    }
    assert_eq!(0, address_a.get_thread(thread_count));

    let mut priv_b = generate_random_private_key();
    let mut pubkey_b = derive_public_key(&priv_b);
    let mut address_b = Address::from_public_key(&pubkey_b).unwrap();
    while 0 != address_b.get_thread(thread_count) {
        priv_b = generate_random_private_key();
        pubkey_b = derive_public_key(&priv_b);
        address_b = Address::from_public_key(&pubkey_b).unwrap();
    }
    assert_eq!(0, address_b.get_thread(thread_count));

    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );

    cfg.t0 = 1000.into();
    cfg.delta_f0 = 32;
    cfg.thread_count = thread_count;
    cfg.operation_validity_periods = 10;
    cfg.operation_batch_size = 3;
    cfg.max_operations_per_block = 50;
    cfg.disable_block_creation = true;

    // to avoid timing pb for block in the future

    let op1 = create_transaction(priv_a, pubkey_a, address_b, 1, 10, 1);
    let op2 = create_transaction(priv_a, pubkey_a, address_b, 2, 10, 1);
    let op3 = create_transaction(priv_a, pubkey_a, address_b, 3, 10, 1);
    let op4 = create_transaction(priv_a, pubkey_a, address_b, 4, 10, 1);
    let op5 = create_transaction(priv_a, pubkey_a, address_b, 5, 10, 1);

    let ops = [
        op1.clone(),
        op2.clone(),
        op3.clone(),
        op4.clone(),
        op5.clone(),
    ];

    let boot_ledger = LedgerSubset(
        vec![(address_a, LedgerData::new(Amount::from_str("100").unwrap()))]
            .into_iter()
            .collect(),
    );

    let (boot_graph, b1, b2) = get_bootgraph(
        derive_public_key(&staking_keys[0]),
        vec![op2.clone(), op3.clone()],
        boot_ledger,
    );
    // there is only one node so it should be drawn at every slot

    cfg.genesis_timestamp = UTime::now(0)
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(4).unwrap())
        .saturating_add(300.into());

    tools::consensus_pool_test(
        cfg.clone(),
        None,
        Some(boot_graph),
        async move |pool_controller,
                    protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver| {
            let ops = consensus_command_sender
                .get_operations(
                    ops.iter()
                        .map(|op| op.get_operation_id().unwrap())
                        .collect(),
                )
                .await
                .unwrap();

            let mut expected = HashMap::new();

            expected.insert(
                op2.get_operation_id().unwrap(),
                OperationSearchResult {
                    status: OperationSearchResultStatus::Pending,
                    op: op2,
                    in_pool: false,
                    in_blocks: vec![(b1, (0, true))].into_iter().collect(),
                },
            );
            expected.insert(
                op3.get_operation_id().unwrap(),
                OperationSearchResult {
                    status: OperationSearchResultStatus::Pending,
                    op: op3,
                    in_pool: false,
                    in_blocks: vec![(b2, (0, false))].into_iter().collect(),
                },
            );

            assert_eq!(ops.len(), expected.len());

            for (
                id,
                OperationSearchResult {
                    op,
                    in_blocks,
                    in_pool,
                    ..
                },
            ) in ops.iter()
            {
                assert!(expected.contains_key(id));
                let OperationSearchResult {
                    op: ex_op,
                    in_pool: ex_pool,
                    in_blocks: ex_blocks,
                    ..
                } = expected.get(id).unwrap();
                assert_eq!(
                    op.get_operation_id().unwrap(),
                    ex_op.get_operation_id().unwrap()
                );
                assert_eq!(in_pool, ex_pool);
                assert_eq!(in_blocks.len(), ex_blocks.len());
                for (b_id, val) in in_blocks.iter() {
                    assert!(ex_blocks.contains_key(b_id));
                    assert_eq!(ex_blocks.get(b_id).unwrap(), val);
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

fn get_bootgraph(
    creator: PublicKey,
    operations: Vec<Operation>,
    ledger: LedgerSubset,
) -> (BootstrapableGraph, BlockId, BlockId) {
    let (genesis_0, g0_id) =
        get_export_active_test_block(creator, vec![], vec![], Slot::new(0, 0), true);
    let (genesis_1, g1_id) =
        get_export_active_test_block(creator, vec![], vec![], Slot::new(0, 1), true);
    let (p1t0, p1t0_id) = get_export_active_test_block(
        creator,
        vec![(g0_id, 0), (g1_id, 0)],
        vec![operations[0].clone()],
        Slot::new(1, 0),
        true,
    );
    let (p1t1, p1t1_id) = get_export_active_test_block(
        creator,
        vec![(g0_id, 0), (g1_id, 0)],
        vec![],
        Slot::new(1, 1),
        false,
    );
    let (p2t0, p2t0_id) = get_export_active_test_block(
        creator,
        vec![(p1t0_id, 1), (p1t1_id, 1)],
        vec![operations[1].clone()],
        Slot::new(2, 0),
        false,
    );
    (
        BootstrapableGraph {
            /// Map of active blocks, where blocks are in their exported version.
            active_blocks: vec![
                (g0_id, genesis_0),
                (g1_id, genesis_1),
                (p1t0_id, p1t0),
                (p1t1_id, p1t1),
                (p2t0_id, p2t0),
            ]
            .into_iter()
            .collect(),
            /// Best parents hashe in each thread.
            best_parents: vec![(p2t0_id, 2), (p1t1_id, 1)],
            /// Latest final period and block hash in each thread.
            latest_final_blocks_periods: vec![(g0_id, 0u64), (g1_id, 0u64)],
            /// Head of the incompatibility graph.
            gi_head: vec![
                (g0_id, Default::default()),
                (p1t0_id, Default::default()),
                (p2t0_id, Default::default()),
                (g1_id, Default::default()),
                (p1t0_id, Default::default()),
                (p2t0_id, Default::default()),
            ]
            .into_iter()
            .collect(),

            /// List of maximal cliques of compatible blocks.
            max_cliques: vec![Clique {
                block_ids: vec![g0_id, p1t0_id, g1_id, p1t1_id, p2t0_id]
                    .into_iter()
                    .collect(),
                fitness: 123,
                is_blockclique: true,
            }],
            ledger,
        },
        p1t0_id,
        p2t0_id,
    )
}
