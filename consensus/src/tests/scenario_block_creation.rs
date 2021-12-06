// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::{
    pos::{RollCounts, RollUpdate, RollUpdates},
    tests::tools::{
        self, create_endorsement, create_roll_transaction, create_transaction, generate_ledger_file,
    },
};
use massa_hash::hash::Hash;
use models::{ledger::LedgerData, EndorsementId};
use models::{Address, Amount, Block, BlockHeader, BlockHeaderContent, Slot};
use models::{Endorsement, SerializeCompact};
use pool::PoolCommand;
use protocol_exports::ProtocolCommand;
use serial_test::serial;
use signature::{derive_public_key, generate_random_private_key, PrivateKey};
use std::collections::HashMap;
use std::str::FromStr;
use time::UTime;
use tokio::time::sleep_until;

#[tokio::test]
#[serial]
async fn test_genesis_block_creation() {
    let thread_count = 2;
    // define addresses use for the test
    // addresses a and b both in thread 0
    // addr 1 has 1 roll and 0 coins
    // addr 2 is in consensus and has 0 roll and 1000 coins
    let mut priv_1 = generate_random_private_key();
    let mut pubkey_1 = derive_public_key(&priv_1);
    let mut address_1 = Address::from_public_key(&pubkey_1).unwrap();
    while 0 != address_1.get_thread(thread_count) {
        priv_1 = generate_random_private_key();
        pubkey_1 = derive_public_key(&priv_1);
        address_1 = Address::from_public_key(&pubkey_1).unwrap();
    }
    assert_eq!(0, address_1.get_thread(thread_count));

    let mut priv_2 = generate_random_private_key();
    let mut pubkey_2 = derive_public_key(&priv_2);
    let mut address_2 = Address::from_public_key(&pubkey_2).unwrap();
    while 0 != address_2.get_thread(thread_count) {
        priv_2 = generate_random_private_key();
        pubkey_2 = derive_public_key(&priv_2);
        address_2 = Address::from_public_key(&pubkey_2).unwrap();
    }
    assert_eq!(0, address_2.get_thread(thread_count));

    let mut ledger = HashMap::new();
    ledger.insert(
        address_2,
        LedgerData::new(Amount::from_str("1000").unwrap()),
    );
    let ledger_file = generate_ledger_file(&ledger);
    let staking_keys: Vec<PrivateKey> = vec![priv_1, priv_2];

    // init roll cont
    let mut roll_counts = RollCounts::default();
    let update = RollUpdate {
        roll_purchases: 1,
        roll_sales: 0,
    };
    let mut updates = RollUpdates::default();
    updates.apply(&address_1, &update).unwrap();
    roll_counts.apply_updates(&updates).unwrap();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);

    let roll_counts_file = tools::generate_roll_counts_file(&roll_counts);
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );

    // Set genesis timestamp.
    cfg.genesis_timestamp = UTime::from_str("1633301290000").unwrap();

    tools::consensus_without_pool_test(
        cfg.clone(),
        async move |protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let _genesis_ids = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .genesis_blocks;

            (
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}

// implement test of issue !424.
#[tokio::test]
#[serial]
async fn test_block_creation_with_draw() {
    let thread_count = 2;
    // define addresses use for the test
    // addresses a and b both in thread 0
    // addr 1 has 1 roll and 0 coins
    // addr 2 is in consensus and has 0 roll and 1000 coins
    let mut priv_1 = generate_random_private_key();
    let mut pubkey_1 = derive_public_key(&priv_1);
    let mut address_1 = Address::from_public_key(&pubkey_1).unwrap();
    while 0 != address_1.get_thread(thread_count) {
        priv_1 = generate_random_private_key();
        pubkey_1 = derive_public_key(&priv_1);
        address_1 = Address::from_public_key(&pubkey_1).unwrap();
    }
    assert_eq!(0, address_1.get_thread(thread_count));

    let mut priv_2 = generate_random_private_key();
    let mut pubkey_2 = derive_public_key(&priv_2);
    let mut address_2 = Address::from_public_key(&pubkey_2).unwrap();
    while 0 != address_2.get_thread(thread_count) {
        priv_2 = generate_random_private_key();
        pubkey_2 = derive_public_key(&priv_2);
        address_2 = Address::from_public_key(&pubkey_2).unwrap();
    }
    assert_eq!(0, address_2.get_thread(thread_count));

    let mut ledger = HashMap::new();
    ledger.insert(
        address_2,
        LedgerData::new(Amount::from_str("1000").unwrap()),
    );
    let ledger_file = generate_ledger_file(&ledger);
    let staking_keys: Vec<PrivateKey> = vec![priv_1, priv_2];

    // init roll cont
    let mut roll_counts = RollCounts::default();
    let update = RollUpdate {
        roll_purchases: 1,
        roll_sales: 0,
    };
    let mut updates = RollUpdates::default();
    updates.apply(&address_1, &update).unwrap();
    roll_counts.apply_updates(&updates).unwrap();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);

    let roll_counts_file = tools::generate_roll_counts_file(&roll_counts);
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.roll_price = Amount::from_str("1000").unwrap();
    cfg.periods_per_cycle = 1_000;
    cfg.t0 = 1000.into();
    cfg.pos_lookback_cycles = 2;
    cfg.thread_count = thread_count;
    cfg.delta_f0 = 3;
    cfg.genesis_timestamp = UTime::now(0)
        .unwrap()
        .checked_sub((cfg.t0.to_millis() * cfg.periods_per_cycle * 3).into())
        .unwrap()
        .checked_add(2000.into())
        .unwrap();
    cfg.block_reward = Amount::default();
    cfg.disable_block_creation = false;
    cfg.operation_validity_periods = 100;
    cfg.operation_batch_size = 3;
    cfg.max_operations_per_block = 50;

    let operation_fee = 0;

    tools::consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let genesis_ids = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .genesis_blocks;

            // initial block: addr2 buys 1 roll
            let op1 = create_roll_transaction(priv_2, pubkey_2, 1, true, 10, operation_fee);
            let (initial_block_id, block, _) = tools::create_block_with_operations(
                &cfg,
                Slot::new(1, 0),
                &genesis_ids,
                staking_keys[0],
                vec![op1],
            );
            tools::propagate_block(&mut protocol_controller, block, true, 1000).await;

            // make cycle 0 final/finished by sending enough blocks in each thread in cycle 1
            // note that blocks in cycle 3 may be created during this, so make sure that their clique is overrun by sending a large amount of blocks
            let mut cur_parents = vec![initial_block_id, genesis_ids[1]];
            for delta_period in 0u64..10 {
                for thread in 0..cfg.thread_count {
                    let res_block_id = tools::create_and_test_block(
                        &mut protocol_controller,
                        &cfg,
                        Slot::new(cfg.periods_per_cycle + delta_period, thread),
                        cur_parents.clone(),
                        true,
                        false,
                        staking_keys[0],
                    )
                    .await;
                    cur_parents[thread as usize] = res_block_id;
                }
            }

            // get draws for cycle 3 (lookback = cycle 0)
            let draws: HashMap<_, _> = consensus_command_sender
                .get_selection_draws(
                    Slot::new(3 * cfg.periods_per_cycle, 0),
                    Slot::new(4 * cfg.periods_per_cycle, 0),
                )
                .await
                .unwrap()
                .into_iter()
                .map(|(s, (b, _e))| (s, b))
                .collect();
            let nb_address1_draws = draws.iter().filter(|(_, addr)| **addr == address_1).count();
            // fair coin test. See https://en.wikipedia.org/wiki/Checking_whether_a_coin_is_fair
            // note: this is a statistical test. It may fail in rare occasions.
            assert!(
                (0.5 - ((nb_address1_draws as f32)
                    / ((cfg.thread_count as u64 * cfg.periods_per_cycle) as f32)))
                    .abs()
                    < 0.15
            );

            // check 10 draws
            let draws: HashMap<Slot, Address> = draws.into_iter().collect();
            let mut cur_slot = Slot::new(cfg.periods_per_cycle * 3, 0);
            for _ in 0..10 {
                // wait block propagation
                let block_creator = protocol_controller
                    .wait_command(3500.into(), |cmd| match cmd {
                        ProtocolCommand::IntegratedBlock { block, .. } => {
                            if block.header.content.slot == cur_slot {
                                Some(block.header.content.creator)
                            } else {
                                None
                            }
                        }
                        _ => None,
                    })
                    .await
                    .expect("block did not propagate in time");
                assert_eq!(
                    draws[&cur_slot],
                    Address::from_public_key(&block_creator).unwrap(),
                    "wrong block creator"
                );
                cur_slot = cur_slot.get_next_slot(cfg.thread_count).unwrap();
            }

            (
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
async fn test_interleaving_block_creation_with_reception() {
    let thread_count = 1;
    // define addresses use for the test
    // addresses a and b both in thread 0
    let mut priv_1 = generate_random_private_key();
    let mut pubkey_1 = derive_public_key(&priv_1);
    let mut address_1 = Address::from_public_key(&pubkey_1).unwrap();
    while 0 != address_1.get_thread(thread_count) {
        priv_1 = generate_random_private_key();
        pubkey_1 = derive_public_key(&priv_1);
        address_1 = Address::from_public_key(&pubkey_1).unwrap();
    }
    assert_eq!(0, address_1.get_thread(thread_count));

    let mut priv_2 = generate_random_private_key();
    let mut pubkey_2 = derive_public_key(&priv_2);
    let mut address_2 = Address::from_public_key(&pubkey_2).unwrap();
    while 0 != address_2.get_thread(thread_count) {
        priv_2 = generate_random_private_key();
        pubkey_2 = derive_public_key(&priv_2);
        address_2 = Address::from_public_key(&pubkey_2).unwrap();
    }
    assert_eq!(0, address_2.get_thread(thread_count));

    let mut ledger = HashMap::new();
    ledger.insert(
        address_2,
        LedgerData::new(Amount::from_str("1000").unwrap()),
    );
    let ledger_file = generate_ledger_file(&ledger);

    //init roll cont
    let mut roll_counts = RollCounts::default();
    let update = RollUpdate {
        roll_purchases: 1,
        roll_sales: 0,
    };
    let mut updates = RollUpdates::default();
    updates.apply(&address_1, &update).unwrap();
    updates.apply(&address_2, &update).unwrap();
    roll_counts.apply_updates(&updates).unwrap();
    let staking_file = tools::generate_staking_keys_file(&vec![priv_1]);

    let roll_counts_file = tools::generate_roll_counts_file(&roll_counts);
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.t0 = 1000.into();
    cfg.thread_count = thread_count;
    cfg.genesis_timestamp = UTime::now(0).unwrap().checked_add(1000.into()).unwrap();
    cfg.disable_block_creation = false;

    tools::consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let mut parents = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .genesis_blocks;

            let draws: HashMap<_, _> = consensus_command_sender
                .get_selection_draws(Slot::new(1, 0), Slot::new(11, 0))
                .await
                .unwrap()
                .into_iter()
                .map(|(s, (b, _e))| (s, b))
                .collect();

            sleep_until(
                cfg.genesis_timestamp
                    .saturating_add(cfg.t0)
                    .saturating_sub(150.into())
                    .estimate_instant(0)
                    .expect("could  not estimate instant for genesis timestamps"),
            )
            .await;

            // check 10 draws
            // Key1 and key2 can be drawn to produce block,
            // but the local node only has key1,
            // so when key2 is selected a block must be produced remotly
            // and sent to the local node through protocol
            for i in 1..11 {
                let cur_slot = Slot::new(i, 0);
                let creator = draws.get(&cur_slot).expect("missing slot in drawss");

                let block_id = if *creator == address_1 {
                    // wait block propagation
                    let (header, id) = protocol_controller
                        .wait_command(cfg.t0.saturating_add(300.into()), |cmd| match cmd {
                            ProtocolCommand::IntegratedBlock {
                                block, block_id, ..
                            } => {
                                if block.header.content.slot == cur_slot {
                                    Some((block.header, block_id))
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        })
                        .await
                        .expect("block did not propagate in time");
                    assert_eq!(
                        *creator,
                        Address::from_public_key(&header.content.creator).unwrap(),
                        "wrong block creator"
                    );
                    id
                } else if *creator == address_2 {
                    // create block and propagate it
                    let (block_id, block, _) = tools::create_block_with_operations(
                        &cfg,
                        cur_slot,
                        &parents,
                        priv_2,
                        vec![],
                    );
                    tools::propagate_block(
                        &mut protocol_controller,
                        block,
                        true,
                        cfg.t0.to_millis() + 300,
                    )
                    .await;
                    block_id
                } else {
                    panic!("unexpected block creator");
                };
                parents[0] = block_id;
            }

            (
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
async fn test_order_of_inclusion() {
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

    let mut ledger = HashMap::new();
    ledger.insert(address_a, LedgerData::new(Amount::from_str("100").unwrap()));
    let ledger_file = generate_ledger_file(&ledger);
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
    cfg.disable_block_creation = false;
    cfg.thread_count = thread_count;
    cfg.operation_validity_periods = 10;
    cfg.operation_batch_size = 3;
    cfg.max_operations_per_block = 50;
    // Increase timestamp a bit to avoid missing the first slot.
    cfg.genesis_timestamp = UTime::now(0).unwrap().checked_add(1000.into()).unwrap();

    let op1 = create_transaction(priv_a, pubkey_a, address_b, 5, 10, 1);
    let op2 = create_transaction(priv_a, pubkey_a, address_b, 50, 10, 10);
    let op3 = create_transaction(priv_b, pubkey_b, address_a, 10, 10, 15);

    // there is only one node so it should be drawn at every slot

    tools::consensus_pool_test(
        cfg.clone(),
        None,
        None,
        async move |mut pool_controller,
                    mut protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver| {
            // wait for first slot
            pool_controller
                .wait_command(cfg.t0.checked_mul(2).unwrap(), |cmd| match cmd {
                    PoolCommand::UpdateCurrentSlot(s) => {
                        if s == Slot::new(1, 0) {
                            Some(())
                        } else {
                            None
                        }
                    }
                    PoolCommand::GetEndorsements { response_tx, .. } => {
                        response_tx.send(Vec::new()).unwrap();
                        None
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
                                (op3.get_operation_id().unwrap(), op3.clone(), 50),
                                (op2.get_operation_id().unwrap(), op2.clone(), 50),
                                (op1.get_operation_id().unwrap(), op1.clone(), 50),
                            ])
                            .unwrap();
                        Some(())
                    }
                    PoolCommand::GetEndorsements { response_tx, .. } => {
                        response_tx.send(Vec::new()).unwrap();
                        None
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
                    PoolCommand::GetEndorsements { response_tx, .. } => {
                        response_tx.send(Vec::new()).unwrap();
                        None
                    }
                    _ => None,
                })
                .await
                .expect("timeout while waiting for 2nd operation batch request");

            // wait for block
            let (_block_id, block) = protocol_controller
                .wait_command(300.into(), |cmd| match cmd {
                    ProtocolCommand::IntegratedBlock {
                        block_id, block, ..
                    } => Some((block_id, block)),
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
                    expected[i].get_operation_id().unwrap(),
                    res[i].get_operation_id().unwrap()
                );
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
async fn test_block_filling() {
    // setup logging
    // stderrlog::new()
    // .verbosity(2)
    // .timestamp(stderrlog::Timestamp::Millisecond)
    // .init()
    // .unwrap();

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
    let mut priv_b = generate_random_private_key();
    let mut pubkey_b = derive_public_key(&priv_b);
    let mut address_b = Address::from_public_key(&pubkey_b).unwrap();
    while 1 != address_b.get_thread(thread_count) {
        priv_b = generate_random_private_key();
        pubkey_b = derive_public_key(&priv_b);
        address_b = Address::from_public_key(&pubkey_b).unwrap();
    }
    let mut ledger = HashMap::new();
    ledger.insert(
        address_a,
        LedgerData::new(Amount::from_str("1000000000").unwrap()),
    );
    let ledger_file = generate_ledger_file(&ledger);
    let staking_keys = vec![priv_a, priv_b];
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.t0 = 1000.into();
    cfg.delta_f0 = 32;
    cfg.disable_block_creation = false;
    cfg.thread_count = thread_count;
    cfg.operation_validity_periods = 10;
    cfg.operation_batch_size = 500;
    cfg.periods_per_cycle = 3;
    cfg.max_operations_per_block = 5000;
    cfg.max_block_size = 2000;
    cfg.endorsement_count = 10;
    let mut ops = Vec::new();
    for _ in 0..500 {
        ops.push(create_transaction(priv_a, pubkey_a, address_a, 5, 10, 1))
    }

    tools::consensus_pool_test(
        cfg.clone(),
        None,
        None,
        async move |mut pool_controller,
                    mut protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver| {
            let op_size = 10;

            // wait for slot
            let mut prev_blocks = Vec::new();
            for cur_slot in [Slot::new(1, 0), Slot::new(1, 1)] {
                pool_controller
                    .wait_command(cfg.t0.checked_mul(2).unwrap(), |cmd| match cmd {
                        PoolCommand::UpdateCurrentSlot(s) => {
                            if s == cur_slot {
                                Some(())
                            } else {
                                None
                            }
                        }
                        _ => None,
                    })
                    .await
                    .expect("timeout while waiting for slot");
                // respond to pool batch command
                pool_controller
                    .wait_command(300.into(), |cmd| match cmd {
                        PoolCommand::GetOperationBatch { response_tx, .. } => {
                            response_tx.send(Default::default()).unwrap();
                            Some(())
                        }
                        _ => None,
                    })
                    .await
                    .expect("timeout while waiting for operation batch request");
                // wait for block
                let (block_id, block) = protocol_controller
                    .wait_command(500.into(), |cmd| match cmd {
                        ProtocolCommand::IntegratedBlock {
                            block_id, block, ..
                        } => Some((block_id, block)),
                        _ => None,
                    })
                    .await
                    .expect("timeout while waiting for block");
                assert_eq!(block.header.content.slot, cur_slot);
                prev_blocks.push(block_id);
            }

            // wait for slot p2t0
            pool_controller
                .wait_command(cfg.t0, |cmd| match cmd {
                    PoolCommand::UpdateCurrentSlot(s) => {
                        if s == Slot::new(2, 0) {
                            Some(())
                        } else {
                            None
                        }
                    }
                    _ => None,
                })
                .await
                .expect("timeout while waiting for slot");

            // respond to endorsement command
            let eds = pool_controller
                .wait_command(300.into(), |cmd| match cmd {
                    PoolCommand::GetEndorsements {
                        target_slot,
                        parent,
                        creators,
                        response_tx,
                        ..
                    } => {
                        assert_eq!(Slot::new(1, 0), target_slot);
                        assert_eq!(parent, prev_blocks[0]);
                        let mut eds: Vec<(EndorsementId, Endorsement)> = Vec::new();
                        for (index, creator) in creators.iter().enumerate() {
                            let ed = if *creator == address_a {
                                create_endorsement(priv_a, target_slot, parent, index as u32)
                            } else if *creator == address_b {
                                create_endorsement(priv_b, target_slot, parent, index as u32)
                            } else {
                                panic!("invalid endorser choice");
                            };
                            eds.push((ed.compute_endorsement_id().unwrap(), ed));
                        }
                        response_tx.send(eds.clone()).unwrap();
                        Some(eds)
                    }
                    _ => None,
                })
                .await
                .expect("timeout while waiting for endorsement request");
            assert_eq!(eds.len() as u32, cfg.endorsement_count);

            // respond to first pool batch command
            pool_controller
                .wait_command(300.into(), |cmd| match cmd {
                    PoolCommand::GetOperationBatch { response_tx, .. } => {
                        response_tx
                            .send(
                                ops.iter()
                                    .map(|op| (op.get_operation_id().unwrap(), op.clone(), op_size))
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
            let (_block_id, block) = protocol_controller
                .wait_command(500.into(), |cmd| match cmd {
                    ProtocolCommand::IntegratedBlock {
                        block_id, block, ..
                    } => Some((block_id, block)),
                    _ => None,
                })
                .await
                .expect("timeout while waiting for block");

            // assert it's the expected block
            assert_eq!(block.header.content.slot, Slot::new(2, 0));

            // assert it includes the sent endorsements
            assert_eq!(block.header.content.endorsements.len(), eds.len());
            for (e_found, (e_expected_id, e_expected)) in
                block.header.content.endorsements.iter().zip(eds.iter())
            {
                assert_eq!(e_found.compute_endorsement_id().unwrap(), *e_expected_id);
                assert_eq!(e_expected.compute_endorsement_id().unwrap(), *e_expected_id);
            }

            // create empty block
            let (_block_id, header) = BlockHeader::new_signed(
                &priv_a,
                BlockHeaderContent {
                    creator: block.header.content.creator,
                    slot: block.header.content.slot,
                    parents: block.header.content.parents.clone(),
                    operation_merkle_root: Hash::from(&Vec::new()[..]),
                    endorsements: eds.iter().map(|(_e_id, endo)| endo.clone()).collect(),
                },
            )
            .unwrap();
            let empty = Block {
                header,
                operations: Vec::new(),
            };
            let remaining_block_space = (cfg.max_block_size as usize)
                .checked_sub(empty.to_bytes_compact().unwrap().len() as usize)
                .unwrap();

            let nb = remaining_block_space / (op_size as usize);
            assert_eq!(block.operations.len(), nb);
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
