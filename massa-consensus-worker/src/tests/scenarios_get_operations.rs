// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::tools::*;
use massa_consensus_exports::ConsensusConfig;

use massa_graph::{ledger::ConsensusLedgerSubset, BootstrapableGraph};
use massa_models::WrappedOperation;
use massa_models::{
    clique::Clique, ledger_models::LedgerData, Amount, BlockId, OperationSearchResult,
    OperationSearchResultStatus, Slot,
};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use serial_test::serial;
use std::collections::HashMap;
use std::str::FromStr;

#[tokio::test]
#[serial]
async fn test_get_operation() {
    // // setup logging
    // stderrlog::new()
    //     .verbosity(4)
    //     .timestamp(stderrlog::Timestamp::Millisecond)
    //     .init()
    //     .unwrap();
    let staking_keys: Vec<KeyPair> = (0..1).map(|_| KeyPair::generate()).collect();
    let cfg = ConsensusConfig {
        t0: 1000.into(),
        operation_validity_periods: 10,
        max_operations_per_block: 50,
        genesis_timestamp: MassaTime::now()
            .unwrap()
            .saturating_sub(MassaTime::from(32000).checked_mul(4).unwrap())
            .saturating_add(300.into()),
        ..ConsensusConfig::default_with_staking_keys(&staking_keys)
    };
    // define addresses use for the test
    // addresses a and b both in thread 0
    let (address_a, keypair_a) = random_address_on_thread(0, cfg.thread_count).into();
    let (address_b, _) = random_address_on_thread(0, cfg.thread_count).into();
    // to avoid timing pb for block in the future

    let op1 = create_transaction(&keypair_a, address_b, 1, 10, 1);
    let op2 = create_transaction(&keypair_a, address_b, 2, 10, 1);
    let op3 = create_transaction(&keypair_a, address_b, 3, 10, 1);
    let op4 = create_transaction(&keypair_a, address_b, 4, 10, 1);
    let op5 = create_transaction(&keypair_a, address_b, 5, 10, 1);

    let ops = [
        op1.clone(),
        op2.clone(),
        op3.clone(),
        op4.clone(),
        op5.clone(),
    ];

    let boot_ledger = ConsensusLedgerSubset(
        vec![(address_a, LedgerData::new(Amount::from_str("100").unwrap()))]
            .into_iter()
            .collect(),
    );

    let (boot_graph, b1, b2) = get_bootgraph(vec![op2.clone(), op3.clone()], boot_ledger);
    // there is only one node so it should be drawn at every slot

    consensus_pool_test(
        cfg.clone(),
        None,
        Some(boot_graph),
        async move |pool_controller,
                    protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver| {
            let ops = consensus_command_sender
                .get_operations(ops.iter().map(|op| op.id).collect())
                .await
                .unwrap();

            let mut expected = HashMap::new();

            expected.insert(
                op2.id,
                OperationSearchResult {
                    status: OperationSearchResultStatus::Pending,
                    op: op2,
                    in_pool: false,
                    in_blocks: vec![(b1, (0, true))].into_iter().collect(),
                },
            );
            expected.insert(
                op3.id,
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
                assert_eq!(op.id, ex_op.id);
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
    operations: Vec<WrappedOperation>,
    ledger: ConsensusLedgerSubset,
) -> (BootstrapableGraph, BlockId, BlockId) {
    let genesis_0 = get_export_active_test_block(vec![], vec![], Slot::new(0, 0), true);
    let genesis_1 = get_export_active_test_block(vec![], vec![], Slot::new(0, 1), true);
    let p1t0 = get_export_active_test_block(
        vec![(genesis_0.block_id, 0), (genesis_1.block_id, 0)],
        vec![operations[0].clone()],
        Slot::new(1, 0),
        true,
    );
    let p1t1 = get_export_active_test_block(
        vec![(genesis_0.block_id, 0), (genesis_1.block_id, 0)],
        vec![],
        Slot::new(1, 1),
        false,
    );
    let p2t0 = get_export_active_test_block(
        vec![(p1t0.block_id, 1), (p1t1.block_id, 1)],
        vec![operations[1].clone()],
        Slot::new(2, 0),
        false,
    );
    (
        BootstrapableGraph {
            /// Map of active blocks, where blocks are in their exported version.
            active_blocks: vec![
                (genesis_0.block_id, genesis_0.clone()),
                (genesis_1.block_id, genesis_1.clone()),
                (p1t0.block_id, p1t0.clone()),
                (p1t1.block_id, p1t1.clone()),
                (p2t0.block_id, p2t0.clone()),
            ]
            .into_iter()
            .collect(),
            /// Best parents hashes in each thread.
            best_parents: vec![(p2t0.block_id, 2), (p1t1.block_id, 1)],
            /// Latest final period and block hash in each thread.
            latest_final_blocks_periods: vec![
                (genesis_0.block_id, 0u64),
                (genesis_1.block_id, 0u64),
            ],
            /// Head of the incompatibility graph.
            gi_head: vec![
                (genesis_0.block_id, Default::default()),
                (p1t0.block_id, Default::default()),
                (p2t0.block_id, Default::default()),
                (genesis_1.block_id, Default::default()),
                (p1t0.block_id, Default::default()),
                (p2t0.block_id, Default::default()),
            ]
            .into_iter()
            .collect(),

            /// List of maximal cliques of compatible blocks.
            max_cliques: vec![Clique {
                block_ids: vec![
                    genesis_0.block_id,
                    p1t0.block_id,
                    genesis_1.block_id,
                    p1t1.block_id,
                    p2t0.block_id,
                ]
                .into_iter()
                .collect(),
                fitness: 123,
                is_blockclique: true,
            }],
            ledger,
        },
        p1t0.block_id,
        p2t0.block_id,
    )
}
