use serial_test::serial;
use std::collections::HashMap;

use communication::protocol::ProtocolCommand;
use crypto::{
    hash::Hash,
    signature::{PrivateKey, PublicKey},
};
use models::SerializeCompact;
use models::{
    get_serialization_context, Address, Block, BlockHeader, BlockHeaderContent, BlockId, Operation,
    SerializationContext, Slot,
};
use pool::PoolCommand;
use time::UTime;
use tokio::time::sleep;

use crate::{
    block_graph::ExportActiveBlock,
    ledger::{LedgerChange, LedgerData, OperationLedgerInterface},
    start_consensus_controller,
    tests::{
        mock_pool_controller::{MockPoolController, PoolCommandSink},
        mock_protocol_controller::MockProtocolController,
        tools::{self, create_transaction, generate_ledger_file},
    },
    BootsrapableGraph, LedgerExport,
};

#[tokio::test]
#[serial]
async fn test_order_of_inclusion() {
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

    let mut ledger = HashMap::new();
    ledger.insert(address_a, LedgerData { balance: 100 });
    let ledger_file = generate_ledger_file(&ledger);
    let mut cfg = tools::default_consensus_config(1, ledger_file.path());
    let serialization_context = get_serialization_context();
    cfg.t0 = 1000.into();
    cfg.delta_f0 = 32;
    cfg.disable_block_creation = false;
    cfg.thread_count = thread_count;
    cfg.operation_validity_periods = 10;
    cfg.operation_batch_size = 3;
    cfg.max_operations_per_block = 50;
    //to avoid timing pb for block in the future
    cfg.genesis_timestamp = UTime::now(0).unwrap();

    let op1 = create_transaction(priv_a, pubkey_a, address_b, 5, 10, 1);
    let op2 = create_transaction(priv_a, pubkey_a, address_b, 50, 10, 10);
    let op3 = create_transaction(priv_b, pubkey_b, address_a, 10, 10, 15);

    // there is only one node so it should be drawn at every slot

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let (mut pool_controller, pool_command_sender) = MockPoolController::new();

    // launch consensus controller
    let (_consensus_command_sender, consensus_event_receiver, consensus_manager) =
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

    //wait for fisrt slot
    pool_controller
        .wait_command(cfg.t0.checked_mul(2).unwrap(), |cmd| match cmd {
            PoolCommand::UpdateCurrentSlot(s) => {
                if s == Slot::new(1, 0) {
                    Some(())
                } else {
                    None
                }
            }
            _ => None,
        })
        .await
        .expect("timeout while waiting for slot");

    // respond to first pool batch command
    pool_controller
        .wait_command(300.into(), |cmd| match cmd {
            PoolCommand::GetOperationBatch { response_tx, .. } => {
                response_tx
                    .send(vec![
                        (
                            op3.get_operation_id(&serialization_context).unwrap(),
                            op3.clone(),
                            50,
                        ),
                        (
                            op2.get_operation_id(&serialization_context).unwrap(),
                            op2.clone(),
                            50,
                        ),
                        (
                            op1.get_operation_id(&serialization_context).unwrap(),
                            op1.clone(),
                            50,
                        ),
                    ])
                    .unwrap();
                Some(())
            }
            _ => None,
        })
        .await
        .expect("timeout while waiting for 1st operation batch request");

    // respond to second pool batch command
    pool_controller
        .wait_command(300.into(), |cmd| match cmd {
            PoolCommand::GetOperationBatch {
                response_tx,
                exclude,
                ..
            } => {
                assert!(!exclude.is_empty());
                response_tx.send(vec![]).unwrap();
                Some(())
            }
            _ => None,
        })
        .await
        .expect("timeout while waiting for 2nd operation batch request");

    // wait for block
    let (_block_id, block) = protocol_controller
        .wait_command(300.into(), |cmd| match cmd {
            ProtocolCommand::IntegratedBlock { block_id, block } => Some((block_id, block)),
            _ => None,
        })
        .await
        .expect("timeout while waiting for block");

    // assert it's the expected block
    assert_eq!(block.header.content.slot, Slot::new(1, 0));
    let expected = vec![op2.clone(), op1.clone()];
    let res = block.operations.clone();
    assert_eq!(block.operations.len(), 2);
    for i in 0..2 {
        assert_eq!(
            expected[i]
                .get_operation_id(&serialization_context)
                .unwrap(),
            res[i].get_operation_id(&serialization_context).unwrap()
        );
    }

    // stop controller while ignoring all commands
    let _pool_sink = PoolCommandSink::new(pool_controller).await;
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}

#[tokio::test]
#[serial]
async fn test_with_two_cliques() {
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
    let mut cfg = tools::default_consensus_config(1, ledger_file.path());
    let serialization_context = models::get_serialization_context();
    cfg.t0 = 1000.into();
    cfg.delta_f0 = 32;
    cfg.disable_block_creation = false;
    cfg.thread_count = thread_count;
    cfg.operation_validity_periods = 10;
    cfg.operation_batch_size = 3;
    cfg.max_operations_per_block = 50;
    //to avoid timing pb for block in the future

    let op1 = create_transaction(priv_a, pubkey_a, address_b, 5, 10, 1);
    let op2 = create_transaction(priv_a, pubkey_a, address_b, 50, 10, 10);
    let op3 = create_transaction(priv_b, pubkey_b, address_a, 10, 10, 15);

    let boot_ledger = LedgerExport {
        ledger_per_thread: vec![vec![(address_a, LedgerData { balance: 100 })], vec![]],
        latest_final_periods: vec![0, 0],
    };

    let boot_graph = get_two_cliques_bootgraph(
        cfg.nodes[0].0,
        &serialization_context,
        vec![op1.clone(), op2.clone(), op3.clone()],
        boot_ledger,
    );
    // there is only one node so it should be drawn at every slot

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let (mut pool_controller, pool_command_sender) = MockPoolController::new();
    cfg.genesis_timestamp = UTime::now(0)
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(4).unwrap())
        .saturating_add(300.into());
    // launch consensus controller
    let (_consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
            pool_command_sender,
            None,
            Some(boot_graph),
            0,
        )
        .await
        .expect("could not start consensus controller");

    //wait for fisrt slot
    pool_controller
        .wait_command(cfg.t0.checked_mul(2).unwrap(), |cmd| match cmd {
            PoolCommand::UpdateCurrentSlot(s) => {
                if s == Slot::new(4, 0) {
                    Some(())
                } else {
                    None
                }
            }
            _ => None,
        })
        .await
        .expect("timeout while waiting for slot");

    // respond to first pool batch command
    pool_controller
        .wait_command(300.into(), |cmd| match cmd {
            PoolCommand::GetOperationBatch { response_tx, .. } => {
                response_tx
                    .send(vec![
                        (
                            op3.get_operation_id(&serialization_context).unwrap(),
                            op3.clone(),
                            50,
                        ),
                        (
                            op2.get_operation_id(&serialization_context).unwrap(),
                            op2.clone(),
                            50,
                        ),
                    ])
                    .unwrap();
                Some(())
            }
            _ => None,
        })
        .await
        .expect("timeout while waiting for 1st operation batch request");

    // respond to second pool batch command
    // no second pool batch request as we sent only 2 operations

    // wait for block
    let (block_id, block) = protocol_controller
        .wait_command(300.into(), |cmd| match cmd {
            ProtocolCommand::IntegratedBlock { block_id, block } => Some((block_id, block)),
            _ => None,
        })
        .await
        .expect("timeout while waiting for block");

    // assert it's the expected block
    assert_eq!(block.header.content.slot, Slot::new(4, 0));
    assert_eq!(block.operations.len(), 1);
    assert_eq!(
        block.operations[0]
            .get_operation_id(&serialization_context)
            .unwrap(),
        op2.get_operation_id(&serialization_context).unwrap()
    );

    // stop controller while ignoring all commands
    let _pool_sink = PoolCommandSink::new(pool_controller).await;
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}

fn get_export_active_test_block(
    creator: PublicKey,
    parents: Vec<(BlockId, u64)>,
    operations: Vec<Operation>,
    slot: Slot,
    context: &SerializationContext,
) -> (ExportActiveBlock, BlockId) {
    let block = Block {
        header: BlockHeader {
            content: BlockHeaderContent{
                creator: creator,
                operation_merkle_root: Hash::hash(&operations.iter().map(|op|{
                    op
                        .get_operation_id(context)
                        .unwrap()
                        .to_bytes()
                        .clone()
                    })
                    .flatten()
                    .collect::<Vec<_>>()[..]),
                parents: parents.iter()
                    .map(|(id,_)| *id)
                    .collect(),
                slot,
            },
            signature: crypto::signature::Signature::from_bs58_check(
                "5f4E3opXPWc3A1gvRVV7DJufvabDfaLkT1GMterpJXqRZ5B7bxPe5LoNzGDQp9LkphQuChBN1R5yEvVJqanbjx7mgLEae"
            ).unwrap()
        },
        operations: operations.clone(),
    };

    let mut block_ledger_change = vec![HashMap::new(); context.parent_count as usize];
    for op in operations.iter() {
        let thread = Address::from_public_key(&op.content.sender_public_key)
            .unwrap()
            .get_thread(context.parent_count);
        let mut changes = op
            .get_changes(
                &Address::from_public_key(&creator).unwrap(),
                context.parent_count,
            )
            .unwrap();
        for i in 0..changes.len() {
            for (address, change) in changes[i].iter_mut() {
                if let Some(old) = block_ledger_change[i].get(address) {
                    change.chain(old).unwrap();
                }
                block_ledger_change[i].insert(address.clone(), change.clone());
            }
        }
    }
    let id = block.header.compute_block_id().unwrap();
    (
        ExportActiveBlock {
            parents,
            dependencies: vec![],
            block,
            children: vec![vec![], vec![]],
            is_final: false,
            block_ledger_change: block_ledger_change
                .iter()
                .map(|map| {
                    map.into_iter()
                        .map(|(a, c)| (a.clone(), c.clone()))
                        .collect()
                })
                .collect(),
        },
        id,
    )
}

fn get_two_cliques_bootgraph(
    creator: PublicKey,
    context: &SerializationContext,
    operations: Vec<Operation>,
    ledger: LedgerExport,
) -> BootsrapableGraph {
    let (genesis_0, g0_id) =
        get_export_active_test_block(creator.clone(), vec![], vec![], Slot::new(0, 0), context);
    let (genesis_1, g1_id) =
        get_export_active_test_block(creator.clone(), vec![], vec![], Slot::new(0, 1), context);
    let (p1t0, p1t0_id) = get_export_active_test_block(
        creator.clone(),
        vec![(g0_id, 0), (g1_id, 0)],
        vec![operations[1].clone()],
        Slot::new(1, 0),
        context,
    );
    let (p1t1, p1t1_id) = get_export_active_test_block(
        creator.clone(),
        vec![(g0_id, 0), (g1_id, 0)],
        vec![],
        Slot::new(1, 1),
        context,
    );
    let (p2t0, p2t0_id) = get_export_active_test_block(
        creator.clone(),
        vec![(g0_id, 0), (g1_id, 0)],
        vec![operations[0].clone()],
        Slot::new(2, 0),
        context,
    );
    let (p2t1, p2t1_id) = get_export_active_test_block(
        creator.clone(),
        vec![(g0_id, 0), (p1t1_id, 1)],
        vec![],
        Slot::new(2, 1),
        context,
    );
    let (p3t0, p3t0_id) = get_export_active_test_block(
        creator.clone(),
        vec![(p2t0_id, 2), (p2t1_id, 2)],
        vec![],
        Slot::new(3, 0),
        context,
    );
    let (p3t1, p3t1_id) = get_export_active_test_block(
        creator.clone(),
        vec![(p2t0_id, 2), (p2t1_id, 2)],
        vec![],
        Slot::new(3, 1),
        context,
    );
    BootsrapableGraph {
        /// Map of active blocks, where blocks are in their exported version.
        active_blocks: vec![
            (g0_id, genesis_0.clone()),
            (g1_id, genesis_1.clone()),
            (p1t0_id, p1t0.clone()),
            (p1t1_id, p1t1.clone()),
            (p2t0_id, p2t0.clone()),
            (p2t1_id, p2t1.clone()),
            (p3t0_id, p3t0.clone()),
            (p3t1_id, p3t1.clone()),
        ]
        .into_iter()
        .collect(),
        /// Best parents hashe in each thread.
        best_parents: vec![p3t0_id, p3t1_id],
        /// Latest final period and block hash in each thread.
        latest_final_blocks_periods: vec![(g0_id, 0u64), (g1_id, 0u64)],
        /// Head of the incompatibility graph.
        gi_head: vec![
            (g0_id, vec![]),
            (p1t0_id, vec![p2t0_id, p3t0_id, p3t1_id]),
            (p2t0_id, vec![p1t0_id]),
            (p3t0_id, vec![p1t0_id]),
            (g1_id, vec![]),
            (p1t0_id, vec![]),
            (p2t0_id, vec![]),
            (p3t1_id, vec![p1t0_id]),
        ]
        .into_iter()
        .collect(),

        /// List of maximal cliques of compatible blocks.
        max_cliques: vec![
            vec![g0_id, p1t0_id, g1_id, p1t1_id, p2t1_id],
            vec![g0_id, p2t0_id, p3t0_id, g1_id, p1t1_id, p2t1_id, p3t1_id],
        ],
        ledger,
    }
}

#[tokio::test]
#[serial]
async fn test_block_filling() {
    // // setup logging
    // stderrlog::new()
    //     .verbosity(2)
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

    let mut ledger = HashMap::new();
    ledger.insert(
        address_a,
        LedgerData {
            balance: 1000_000_000,
        },
    );
    let ledger_file = generate_ledger_file(&ledger);
    let mut cfg = tools::default_consensus_config(1, ledger_file.path());
    let serialization_context = get_serialization_context();
    cfg.t0 = 1000.into();
    cfg.delta_f0 = 32;
    cfg.disable_block_creation = false;
    cfg.thread_count = thread_count;
    cfg.operation_validity_periods = 10;
    cfg.operation_batch_size = 500;
    cfg.max_operations_per_block = 5000;
    cfg.max_block_size = 500;
    //to avoid timing pb for block in the future
    cfg.genesis_timestamp = UTime::now(0).unwrap();
    let mut ops = Vec::new();
    for _ in 0..500 {
        ops.push(create_transaction(priv_a, pubkey_a, address_a, 5, 10, 1))
    }

    // there is only one node so it should be drawn at every slot

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let (mut pool_controller, pool_command_sender) = MockPoolController::new();

    // launch consensus controller
    let (_consensus_command_sender, consensus_event_receiver, consensus_manager) =
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

    let op_size = 10;

    //wait for fisrt slot
    pool_controller
        .wait_command(cfg.t0.checked_mul(2).unwrap(), |cmd| match cmd {
            PoolCommand::UpdateCurrentSlot(s) => {
                if s == Slot::new(1, 0) {
                    Some(())
                } else {
                    None
                }
            }
            _ => None,
        })
        .await
        .expect("timeout while waiting for slot");

    // respond to first pool batch command
    pool_controller
        .wait_command(300.into(), |cmd| match cmd {
            PoolCommand::GetOperationBatch { response_tx, .. } => {
                response_tx
                    .send(
                        ops.iter()
                            .map(|op| {
                                (
                                    op.get_operation_id(&serialization_context).unwrap(),
                                    op.clone(),
                                    op_size,
                                )
                            })
                            .collect(),
                    )
                    .unwrap();
                Some(())
            }
            _ => None,
        })
        .await
        .expect("timeout while waiting for 1st operation batch request");

    // respond to second pool batch command
    pool_controller
        .wait_command(300.into(), |cmd| match cmd {
            PoolCommand::GetOperationBatch {
                response_tx,
                exclude,
                ..
            } => {
                assert!(!exclude.is_empty());
                response_tx.send(vec![]).unwrap();
                Some(())
            }
            _ => None,
        })
        .await
        .expect("timeout while waiting for 2nd operation batch request");

    // wait for block
    let (block_id, block) = protocol_controller
        .wait_command(300.into(), |cmd| match cmd {
            ProtocolCommand::IntegratedBlock { block_id, block } => Some((block_id, block)),
            _ => None,
        })
        .await
        .expect("timeout while waiting for block");

    // assert it's the expected block
    // assert it's the expected block
    assert_eq!(block.header.content.slot, Slot::new(1, 0));
    // create empty block
    let (_block_id, header) = BlockHeader::new_signed(
        &priv_a,
        BlockHeaderContent {
            creator: block.header.content.creator,
            slot: block.header.content.slot,
            parents: block.header.content.parents.clone(),
            operation_merkle_root: Hash::hash(&Vec::new()[..]),
        },
    )
    .unwrap();
    let empty = Block {
        header,
        operations: Vec::new(),
    };
    let remaining_block_space = (cfg.max_block_size as usize)
        .checked_sub(
            empty
                .to_bytes_compact(&serialization_context)
                .unwrap()
                .len() as usize,
        )
        .unwrap();

    let nb = remaining_block_space / (op_size as usize);
    assert_eq!(block.operations.len(), nb);

    // stop controller while ignoring all commands
    let _pool_sink = PoolCommandSink::new(pool_controller).await;
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}
