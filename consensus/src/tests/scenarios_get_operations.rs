// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::{
    block_graph::ExportClique,
    tests::tools::{self, create_transaction, generate_ledger_file, get_export_active_test_block},
    BootstrapableGraph, LedgerExport,
};
use crypto::{hash::Hash, signature::PublicKey};
use models::ledger::LedgerData;
use models::{
    Address, Amount, Block, BlockHeader, BlockHeaderContent, BlockId, Operation, OperationId,
    OperationSearchResult, OperationSearchResultStatus, Slot,
};
use serial_test::serial;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use time::UTime;

#[tokio::test]
#[serial]
async fn test_storage() {
    // // setup logging
    // stderrlog::new()
    //     .verbosity(4)
    //     .timestamp(stderrlog::Timestamp::Millisecond)
    //     .init()
    //     .unwrap();
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
    cfg.delta_f0 = 32;
    cfg.thread_count = thread_count;
    cfg.operation_validity_periods = 10;
    cfg.operation_batch_size = 3;
    cfg.max_operations_per_block = 50;
    cfg.disable_block_creation = true;

    //to avoid timing pb for block in the future

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

    let boot_ledger = LedgerExport {
        ledger_subset: vec![(address_a, LedgerData::new(Amount::from_str("100").unwrap()))],
    };

    let (boot_graph, b1, b2) = get_bootgraph(
        crypto::derive_public_key(&staking_keys[0]),
        vec![op2.clone(), op3.clone()],
        boot_ledger,
    );
    // there is only one node so it should be drawn at every slot

    // start storage
    let storage_access = tools::start_storage();

    let block = Block {
        header: BlockHeader {
            content: BlockHeaderContent{
                creator: crypto::derive_public_key(&staking_keys[0]),
                operation_merkle_root: Hash::hash(&vec![op4.clone()].iter().map(|op|{
                    op
                        .get_operation_id()
                        .unwrap()
                        .to_bytes()
                        .clone()
                    })
                    .flatten()
                    .collect::<Vec<_>>()[..]),
                parents: vec![0,1].iter()
                    .map(|idx| BlockId(Hash::hash(format!("parent {:?}", idx).as_bytes())))
                    .collect(),
                slot: Slot::new(1,1),
                endorsements: Vec::new(),
            },
            signature: crypto::signature::Signature::from_bs58_check(
                "5f4E3opXPWc3A1gvRVV7DJufvabDfaLkT1GMterpJXqRZ5B7bxPe5LoNzGDQp9LkphQuChBN1R5yEvVJqanbjx7mgLEae"
            ).unwrap()
        },
        operations: vec![op4.clone()],
    };

    let b3 = block.header.compute_block_id().unwrap();
    storage_access.add_block(b3, block).await.unwrap();
    cfg.genesis_timestamp = UTime::now(0)
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(4).unwrap())
        .saturating_add(300.into());

    tools::consensus_pool_test(
        cfg.clone(),
        Some(storage_access),
        None,
        Some(boot_graph),
        async move |mut pool_controller,
                    protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver| {
            let (ops, pool) = tokio::join!(
                consensus_command_sender.get_operations(
                    ops.iter()
                        .map(|op| { op.get_operation_id().unwrap() })
                        .collect()
                ),
                pool_controller.wait_command(2000.into(), |cmd| {
                    match cmd {
                        pool::PoolCommand::GetOperations {
                            operation_ids,
                            response_tx,
                        } => {
                            assert_eq!(
                                operation_ids,
                                ops.iter()
                                    .map(|op| { op.get_operation_id().unwrap() })
                                    .collect()
                            );
                            response_tx
                                .send(
                                    ops[0..4]
                                        .iter()
                                        .map(|op| (op.get_operation_id().unwrap(), op.clone()))
                                        .collect(),
                                )
                                .unwrap();
                            Some(())
                        }
                        _ => None,
                    }
                })
            );

            assert_eq!(pool, Some(()));

            let mut expected = HashMap::new();
            expected.insert(
                op1.get_operation_id().unwrap(),
                OperationSearchResult {
                    status: OperationSearchResultStatus::Pending,
                    op: op1,
                    in_pool: true,
                    in_blocks: HashMap::new(),
                },
            );
            expected.insert(
                op2.get_operation_id().unwrap(),
                OperationSearchResult {
                    status: OperationSearchResultStatus::Pending,
                    op: op2,
                    in_pool: true,
                    in_blocks: vec![(b1, (0, true))].into_iter().collect(),
                },
            );
            expected.insert(
                op3.get_operation_id().unwrap(),
                OperationSearchResult {
                    status: OperationSearchResultStatus::Pending,
                    op: op3,
                    in_pool: true,
                    in_blocks: vec![(b2, (0, false))].into_iter().collect(),
                },
            );
            expected.insert(
                op4.get_operation_id().unwrap(),
                OperationSearchResult {
                    status: OperationSearchResultStatus::Pending,
                    op: op4,
                    in_pool: true,
                    in_blocks: vec![(b3, (0, true))].into_iter().collect(),
                },
            );

            let ops = ops.unwrap();
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

#[tokio::test]
#[serial]
async fn test_consensus_and_storage() {
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
    cfg.delta_f0 = 32;
    cfg.thread_count = thread_count;
    cfg.operation_validity_periods = 10;
    cfg.operation_batch_size = 3;
    cfg.max_operations_per_block = 50;
    cfg.disable_block_creation = true;

    // Consensus: from B to B
    let op_consensus_1 = create_transaction(priv_b, pubkey_b, address_b, 1, 10, 1);

    // Consensus: from A to A
    let op_consensus_2 = create_transaction(priv_a, pubkey_a, address_a, 2, 10, 1);

    // Consensus: from A to B
    let op_consensus_3 = create_transaction(priv_a, pubkey_a, address_b, 3, 10, 1);

    // Pool: involving A only
    let op_pool_1 = create_transaction(priv_a, pubkey_a, address_a, 1, 10, 1);

    // Pool: involving B only
    let op_pool_2 = create_transaction(priv_b, pubkey_b, address_b, 2, 10, 1);

    // Pool: involving A and B
    let op_pool_3 = create_transaction(priv_a, pubkey_a, address_b, 3, 10, 1);

    // Storage: from B to B
    let op_storage_1 = create_transaction(priv_b, pubkey_b, address_b, 1, 10, 1);

    // Storage: from A to A
    let op_storage_2 = create_transaction(priv_a, pubkey_a, address_a, 2, 10, 1);

    // Storage: from A to B
    let op_storage_3 = create_transaction(priv_a, pubkey_a, address_b, 3, 10, 1);

    let boot_ledger = LedgerExport {
        ledger_subset: vec![
            (
                address_a,
                LedgerData::new(Amount::from_str("1000").unwrap()),
            ),
            (
                address_b,
                LedgerData::new(Amount::from_str("1000").unwrap()),
            ),
        ],
    };

    let block = Block {
        header: BlockHeader {
            content: BlockHeaderContent{
                creator: pubkey_a.clone(),
                operation_merkle_root: Hash::hash(&vec![op_storage_1.clone(), op_storage_2.clone(), op_storage_3.clone()].iter().map(|op|{
                    op
                        .get_operation_id()
                        .unwrap()
                        .to_bytes()
                        .clone()
                    })
                    .flatten()
                    .collect::<Vec<_>>()[..]),
                parents: vec![0,1].iter()
                    .map(|idx| BlockId(Hash::hash(format!("parent {:?}", idx).as_bytes())))
                    .collect(),
                slot: Slot::new(1,1),
                endorsements: Vec::new(),
            },
            signature: crypto::signature::Signature::from_bs58_check(
                "5f4E3opXPWc3A1gvRVV7DJufvabDfaLkT1GMterpJXqRZ5B7bxPe5LoNzGDQp9LkphQuChBN1R5yEvVJqanbjx7mgLEae"
            ).unwrap()
        },
        operations: vec![op_storage_1.clone(), op_storage_2.clone(), op_storage_3.clone()],
    };

    // start storage, and add the block containing the "storage ops" to it.
    let storage_access = tools::start_storage();
    let b3 = block.header.compute_block_id().unwrap();
    storage_access.add_block(b3, block).await.unwrap();

    let (boot_graph, _, _) = get_bootgraph(
        pubkey_a.clone(),
        vec![
            op_consensus_1.clone(),
            op_consensus_2.clone(),
            op_consensus_3.clone(),
        ],
        boot_ledger,
    );
    cfg.genesis_timestamp = UTime::now(0)
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(4).unwrap())
        .saturating_add(300.into());

    tools::consensus_pool_test(
        cfg.clone(),
        Some(storage_access),
        None,
        Some(boot_graph),
        async move |mut pool_controller,
                    protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver| {
            // Ask for ops related to A.
            let (ops, pool) = tokio::join!(
                consensus_command_sender.get_operations_involving_address(address_a.clone()),
                pool_controller.wait_command(2000.into(), |cmd| {
                    match cmd {
                        pool::PoolCommand::GetRecentOperations {
                            address,
                            response_tx,
                        } => {
                            assert_eq!(address, address_a);
                            response_tx
                                .send(
                                    vec![op_pool_1.clone(), op_pool_3.clone()]
                                        .into_iter()
                                        .map(|op| {
                                            (
                                                op.get_operation_id().unwrap(),
                                                OperationSearchResult {
                                                    status: OperationSearchResultStatus::Pending,
                                                    op,
                                                    in_pool: true,
                                                    in_blocks: HashMap::new(),
                                                },
                                            )
                                        })
                                        .collect(),
                                )
                                .unwrap();
                            Some(())
                        }
                        _ => None,
                    }
                })
            );

            assert_eq!(pool, Some(()));

            // Check that we received all expected ops related to A.
            let res: HashSet<OperationId> = ops.unwrap().keys().map(|key| key.clone()).collect();
            let expected: HashSet<OperationId> = vec![
                op_consensus_1.clone(),
                op_consensus_2.clone(),
                op_consensus_3.clone(),
                op_pool_1.clone(),
                op_pool_3.clone(),
                op_storage_1.clone(),
                op_storage_2.clone(),
                op_storage_3.clone(),
            ]
            .into_iter()
            .map(|op| op.get_operation_id().unwrap())
            .collect();
            assert_eq!(res, expected);

            // Ask for ops related to B.
            let (ops, pool) = tokio::join!(
                consensus_command_sender.get_operations_involving_address(address_b.clone()),
                pool_controller.wait_command(2000.into(), |cmd| {
                    match cmd {
                        pool::PoolCommand::GetRecentOperations {
                            address,
                            response_tx,
                        } => {
                            assert_eq!(address, address_b);
                            response_tx
                                .send(
                                    vec![op_pool_2.clone(), op_pool_3.clone()]
                                        .into_iter()
                                        .map(|op| {
                                            (
                                                op.get_operation_id().unwrap(),
                                                OperationSearchResult {
                                                    status: OperationSearchResultStatus::Pending,
                                                    op,
                                                    in_pool: true,
                                                    in_blocks: HashMap::new(),
                                                },
                                            )
                                        })
                                        .collect(),
                                )
                                .unwrap();
                            Some(())
                        }
                        _ => None,
                    }
                })
            );

            assert_eq!(pool, Some(()));

            // Check that we received all expected ops related to B.
            let res: HashSet<OperationId> = ops.unwrap().keys().map(|key| key.clone()).collect();
            let expected: HashSet<OperationId> = vec![
                op_consensus_1.clone(),
                op_consensus_3.clone(),
                op_pool_2.clone(),
                op_pool_3.clone(),
                op_storage_1.clone(),
                op_storage_3.clone(),
            ]
            .into_iter()
            .map(|op| op.get_operation_id().unwrap())
            .collect();
            assert_eq!(res, expected);
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
    ledger: LedgerExport,
) -> (BootstrapableGraph, BlockId, BlockId) {
    let (genesis_0, g0_id) =
        get_export_active_test_block(creator.clone(), vec![], vec![], Slot::new(0, 0), true);
    let (genesis_1, g1_id) =
        get_export_active_test_block(creator.clone(), vec![], vec![], Slot::new(0, 1), true);
    let (p1t0, p1t0_id) = get_export_active_test_block(
        creator.clone(),
        vec![(g0_id, 0), (g1_id, 0)],
        vec![operations[0].clone()],
        Slot::new(1, 0),
        true,
    );
    let (p1t1, p1t1_id) = get_export_active_test_block(
        creator.clone(),
        vec![(g0_id, 0), (g1_id, 0)],
        vec![],
        Slot::new(1, 1),
        false,
    );
    let (p2t0, p2t0_id) = get_export_active_test_block(
        creator.clone(),
        vec![(p1t0_id, 1), (p1t1_id, 1)],
        vec![operations[1].clone()],
        Slot::new(2, 0),
        false,
    );
    (
        BootstrapableGraph {
            /// Map of active blocks, where blocks are in their exported version.
            active_blocks: vec![
                (g0_id, genesis_0.clone()),
                (g1_id, genesis_1.clone()),
                (p1t0_id, p1t0.clone()),
                (p1t1_id, p1t1.clone()),
                (p2t0_id, p2t0.clone()),
            ]
            .into_iter()
            .collect(),
            /// Best parents hashe in each thread.
            best_parents: vec![(p2t0_id, 2), (p1t1_id, 1)],
            /// Latest final period and block hash in each thread.
            latest_final_blocks_periods: vec![(g0_id, 0u64), (g1_id, 0u64)],
            /// Head of the incompatibility graph.
            gi_head: vec![
                (g0_id, vec![]),
                (p1t0_id, vec![]),
                (p2t0_id, vec![]),
                (g1_id, vec![]),
                (p1t0_id, vec![]),
                (p2t0_id, vec![]),
            ]
            .into_iter()
            .collect(),

            /// List of maximal cliques of compatible blocks.
            max_cliques: vec![ExportClique {
                block_ids: vec![g0_id, p1t0_id, g1_id, p1t1_id, p2t0_id],
                fitness: 123,
                is_blockclique: true,
            }],
            ledger,
        },
        p1t0_id,
        p2t0_id,
    )
}
