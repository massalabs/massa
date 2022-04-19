// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::tools::{create_executesc, random_address_on_thread};
use crate::tests::tools::{self, create_endorsement, create_roll_transaction, create_transaction};
use massa_consensus_exports::{tools::*, ConsensusConfig};
use massa_graph::ledger::LedgerSubset;
use massa_hash::Hash;
use massa_models::rolls::{RollCounts, RollUpdate, RollUpdates};
use massa_models::signed::{Signable, Signed};
use massa_models::SerializeCompact;
use massa_models::{ledger_models::LedgerData, EndorsementId, OperationType};
use massa_models::{Address, Amount, Block, BlockHeader, SignedEndorsement, Slot};
use massa_pool::PoolCommand;
use massa_protocol_exports::ProtocolCommand;
use massa_signature::{generate_random_private_key, PrivateKey};
use massa_time::MassaTime;
use serial_test::serial;
use std::collections::HashMap;
use std::str::FromStr;
use tokio::time::sleep_until;

#[tokio::test]
#[serial]
async fn test_genesis_block_creation() {
    // define addresses use for the test
    // addresses a and b both in thread 0
    // addr 1 has 1 roll and 0 coins
    // addr 2 is in consensus and has 0 roll and 1000 coins
    let thread_count = 2;
    let (address_1, priv_1, _) = random_address_on_thread(0, thread_count).into();
    let (address_2, priv_2, _) = random_address_on_thread(0, thread_count).into();
    let mut ledger = HashMap::new();
    ledger.insert(
        address_2,
        LedgerData::new(Amount::from_str("1000").unwrap()),
    );
    let mut cfg = ConsensusConfig {
        genesis_timestamp: MassaTime::now()
            .unwrap()
            .saturating_sub(MassaTime::from(30000)),
        ..ConsensusConfig::default_with_staking_keys_and_ledger(&[priv_1, priv_2], &ledger)
    };
    // init roll count
    let mut roll_counts = RollCounts::default();
    let update = RollUpdate {
        roll_purchases: 1,
        roll_sales: 0,
    };
    let mut updates = RollUpdates::default();
    updates.apply(&address_1, &update).unwrap();
    roll_counts.apply_updates(&updates).unwrap();

    let initial_rolls_file = generate_roll_counts_file(&roll_counts);
    cfg.initial_rolls_path = initial_rolls_file.path().to_path_buf();

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

/// /// See the test removed at https://gitlab.com/massalabs/massa-network/-/merge_requests/381/diffs#a5bee3b1b5cc9d8157b6feee0ac3e775aa457a33_544_539
///
/// **NOTE: that test is expected to fail 1 / 1000 times**
///
///
/// ### Context
/// ```
/// * price per roll = 1000
/// * periods per cycle = 30 000
/// * t0 = 500ms
/// * look-back = 2
/// * thread count = 2
/// * delta f0 = 3
/// * genesis timestamp = now - t0 * periods per cycle * 3 - 1000
/// * block reward = 0
/// * fee = 0 for every operation
/// * address 1 has 1 roll and 0 coins
/// * address 2 is in consensus and has 0 roll and 1000 coins
/// ```
/// ### Initialization
/// Following blocks are sent through a protocol event to consensus right at the beginning. They all have best parents as parents.
/// * block at slot(1,0) with operation address 2 buys 1 roll
/// * block at slot( period per cycle, 0)
/// * block at slot( period per cycle, 1)
/// * block at slot( period per cycle + 1, 0)
/// * block at slot( period per cycle + 1, 1)
/// * block at slot( period per cycle + 2, 0)
/// * block at slot( period per cycle + 2, 0)
///
/// ### Scenario
///
/// * start consensus
/// * blocks previously described are sent to consensus through a protocol event
/// * assert they are propagated
/// * ```let draws = get_selection_draws( (3*periods_per cycle, 0), (4*periods_per cycle, 0)```
/// * assert
/// ```math
/// abs(1/2 - \frac{TimesAddr1WasDrawn}{ThreadCount * PeriodsPerCycle}) < 0.01
/// ```
/// (see [the math](https://en.wikipedia.org/wiki/Checking_whether_a_coin_is_fair))
/// * wait for cycle 3 beginning
/// * for the 10 first slots of cycle 3
///    * if address 2 was selected assert consensus created and propagated a block
///    * if address 1 was selected assert nothing is propagated
#[tokio::test]
#[serial]
async fn test_block_creation_with_draw() {
    let thread_count = 2;
    // define addresses use for the test
    // addresses a and b both in thread 0
    // addr 1 has 1 roll and 0 coins
    // addr 2 is in consensus and has 0 roll and 1000 coins
    let (address_1, priv_1, _) = random_address_on_thread(0, thread_count).into();
    let (address_2, priv_2, pubkey_2) = random_address_on_thread(0, thread_count).into();

    let staking_keys = vec![priv_1, priv_2];

    // init address_2 with 1000 coins
    let mut ledger = HashMap::new();
    ledger.insert(
        address_2,
        LedgerData::new(Amount::from_str("1000").unwrap()),
    );

    // finally create the configuration
    let t0 = MassaTime::from(1000);
    let periods_per_cycle = 1000;
    let mut cfg = ConsensusConfig {
        block_reward: Amount::default(),
        delta_f0: 3,
        disable_block_creation: false,
        max_operations_per_block: 50,
        operation_validity_periods: 100,
        periods_per_cycle,
        roll_price: Amount::from_str("1000").unwrap(),
        t0,
        genesis_timestamp: MassaTime::now()
            .unwrap()
            .checked_sub((t0.to_millis() * periods_per_cycle * 3).into())
            .unwrap()
            .checked_add(2000.into())
            .unwrap(),
        ..ConsensusConfig::default_with_staking_keys_and_ledger(&staking_keys, &ledger)
    };

    // init roll count
    let mut roll_counts = RollCounts::default();
    let update = RollUpdate {
        roll_purchases: 1,
        roll_sales: 0,
    };
    let mut updates = RollUpdates::default();
    updates.apply(&address_1, &update).unwrap();
    roll_counts.apply_updates(&updates).unwrap();
    let initial_rolls_file = generate_roll_counts_file(&roll_counts);
    cfg.initial_rolls_path = initial_rolls_file.path().to_path_buf();

    let operation_fee = 0;
    tools::consensus_without_pool_test_with_storage(
        cfg.clone(),
        async move |mut protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver,
                    storage| {
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
                        ProtocolCommand::IntegratedBlock { block_id, .. } => {
                            let block = storage
                                .retrieve_block(&block_id)
                                .expect(&format!("Block id : {} not found in storage", block_id));
                            let stored_block = block.read();
                            if stored_block.block.header.content.slot == cur_slot {
                                Some(stored_block.block.header.content.creator)
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
                    Address::from_public_key(&block_creator),
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

/// https://gitlab.com/massalabs/massa/-/issues/301
///
/// Block creation reception mix test
///
/// see https://gitlab.com/massalabs/massa/-/issues/295#note_693561778
///
///
///     two staking keys. Only key a is registered in consensus
///     start before genesis timestamp
///     retrieve next draws
///     for a few slots:
///         if it's key b time to create a block create it and send it to consensus
///         if key a created a block, assert it has chosen as parents expected blocks (no misses), and that it was sent to protocol around the time it was expected.
#[tokio::test]
#[serial]
async fn test_interleaving_block_creation_with_reception() {
    let thread_count = 1;
    // define addresses use for the test
    // addresses a and b both in thread 0
    let (address_1, priv_1, _) = random_address_on_thread(0, thread_count).into();
    let (address_2, priv_2, _) = random_address_on_thread(0, thread_count).into();

    let mut ledger = HashMap::new();
    ledger.insert(address_2, LedgerData::new(Amount::from_raw(1000)));
    let mut cfg = ConsensusConfig {
        thread_count,
        t0: 1000.into(),
        genesis_timestamp: MassaTime::now().unwrap().checked_add(1000.into()).unwrap(),
        disable_block_creation: false,
        ..ConsensusConfig::default_with_staking_keys_and_ledger(&[priv_1], &ledger)
    };
    serde_json::from_str::<LedgerSubset>(
        &tokio::fs::read_to_string(&cfg.initial_ledger_path)
            .await
            .unwrap(),
    )
    .unwrap();
    println!(
        "init ledger path {} {}",
        cfg.initial_ledger_path.to_str().unwrap(),
        std::env::current_dir().unwrap().to_str().unwrap()
    );
    // init roll count
    let mut roll_counts = RollCounts::default();
    let update = RollUpdate {
        roll_purchases: 1,
        roll_sales: 0,
    };
    let mut updates = RollUpdates::default();
    updates.apply(&address_1, &update).unwrap();
    updates.apply(&address_2, &update).unwrap();
    roll_counts.apply_updates(&updates).unwrap();
    let temp_roll_file = generate_roll_counts_file(&roll_counts);
    cfg.initial_rolls_path = temp_roll_file.path().to_path_buf();

    tools::consensus_without_pool_test_with_storage(
        cfg.clone(),
        async move |mut protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver,
                    storage| {
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
                            ProtocolCommand::IntegratedBlock { block_id, .. } => {
                                let block = storage.retrieve_block(&block_id).expect(&format!(
                                    "Block id : {} not found in storage",
                                    block_id
                                ));
                                let stored_block = block.read();
                                if stored_block.block.header.content.slot == cur_slot {
                                    Some((stored_block.block.header.clone(), block_id))
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
                        Address::from_public_key(&header.content.creator),
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

/// https://gitlab.com/massalabs/massa-network-archive/-/issues/343
/// Test block creation with operations
///
/// Test consensus block creation with an initial graph and simulated pool
///
/// In all tests, once it has started there is only one block creator, so we expect consensus to create blocks at every slots after initialization.
///
/// context
///
/// ```
/// initial ledger: A:100
/// op1 : A -> B : 5, fee 1
/// op2 : A -> B : 50, fee 10
/// op3 : B -> A : 10, fee 15
/// ```
///
/// ---
///
/// ```
/// create block at (0,1)
/// operations should be [op2, op1]
/// ```
#[tokio::test]
#[serial]
async fn test_order_of_inclusion() {
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    // Increase timestamp a bit to avoid missing the first slot.
    let init_time: MassaTime = 1000.into();
    let mut cfg = ConsensusConfig {
        disable_block_creation: false,
        genesis_timestamp: MassaTime::now().unwrap().checked_add(init_time).unwrap(),
        max_operations_per_block: 50,
        operation_validity_periods: 10,
        t0: 1000.into(),
        ..ConsensusConfig::default_with_staking_keys(&staking_keys)
    };
    // define addresses use for the test
    // addresses a and b both in thread 0
    let (address_a, priv_a, pubkey_a) = random_address_on_thread(0, cfg.thread_count).into();
    let (address_b, priv_b, pubkey_b) = random_address_on_thread(0, cfg.thread_count).into();

    let mut ledger = HashMap::new();
    ledger.insert(address_a, LedgerData::new(Amount::from_str("100").unwrap()));
    let initial_ledger_file = generate_ledger_file(&ledger); // don't drop the `NamedTempFile`
    cfg.initial_ledger_path = initial_ledger_file.path().to_path_buf();

    let op1 = create_transaction(priv_a, pubkey_a, address_b, 5, 10, 1);
    let op2 = create_transaction(priv_a, pubkey_a, address_b, 50, 10, 10);
    let op3 = create_transaction(priv_b, pubkey_b, address_a, 10, 10, 15);

    // there is only one node so it should be drawn at every slot

    tools::consensus_pool_test_with_storage(
        cfg.clone(),
        None,
        None,
        async move |mut pool_controller,
                    mut protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver,
                    storage| {
            // wait for first slot
            pool_controller
                .wait_command(
                    cfg.t0.saturating_mul(2).saturating_add(init_time),
                    |cmd| match cmd {
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
                    },
                )
                .await
                .expect("timeout while waiting for slot");

            // respond to first pool batch command
            pool_controller
                .wait_command(300.into(), |cmd| match cmd {
                    PoolCommand::GetOperationBatch { response_tx, .. } => {
                        response_tx
                            .send(vec![
                                (op3.content.compute_id().unwrap(), op3.clone(), 50),
                                (op2.content.compute_id().unwrap(), op2.clone(), 50),
                                (op1.content.compute_id().unwrap(), op1.clone(), 50),
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
                    ProtocolCommand::IntegratedBlock { block_id, .. } => {
                        let block = storage
                            .retrieve_block(&block_id)
                            .expect(&format!("Block id : {} not found in storage", block_id));
                        let stored_block = block.read();
                        Some((block_id, stored_block.block.clone()))
                    }
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
                    expected[i].content.compute_id().unwrap(),
                    res[i].content.compute_id().unwrap()
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

/// https://gitlab.com/massalabs/massa-network-archive/-/issues/343
/// Test block creation with operations
///
/// Test consensus block creation with an initial graph and simulated pool
///
/// In all tests, once it has started there is only one block creator, so we expect consensus to create blocks at every slots after initialization.
///
/// context
///
/// ````
/// initial ledger A = 1 000 000
/// max_block_size = 500
/// max_operations_per_block = 10 000
/// op_i = A -> B : 10, 1, signed for the i-th time
/// ```
///
/// ---
/// ```
/// let block_size = size of dummy block at (1,0) without any operation
/// let op_size = size of an operation
/// while consensus is asking for operations send next ops
/// assert created_block_size is max_block_size +/- one op_size
/// assert created_block_size = block_size +`op_size * op_count
/// ```
#[tokio::test]
#[serial]
async fn test_block_filling() {
    let thread_count = 2;
    // define addresses use for the test
    // addresses a and b both in thread 0
    let (address_a, priv_a, pubkey_a) = random_address_on_thread(0, thread_count).into();
    let (address_b, priv_b, _) = random_address_on_thread(0, thread_count).into();
    let mut ledger = HashMap::new();
    ledger.insert(
        address_a,
        LedgerData::new(Amount::from_str("1000000000").unwrap()),
    );
    let cfg = ConsensusConfig {
        disable_block_creation: false,
        endorsement_count: 10,
        max_block_size: 2000,
        max_operations_per_block: 5000,
        operation_batch_size: 500,
        operation_validity_periods: 10,
        periods_per_cycle: 3,
        t0: 1000.into(),
        ..ConsensusConfig::default_with_staking_keys_and_ledger(&[priv_a, priv_b], &ledger)
    };

    let mut ops = vec![create_executesc(
        priv_a,
        pubkey_a,
        10,
        10,
        vec![1; 200], // dummy bytes as here we do not test the content
        1_000,
        0,
        1,
    )]; // this operation has an higher rentability than any other

    for _ in 0..500 {
        ops.push(create_transaction(priv_a, pubkey_a, address_a, 5, 10, 1))
    }

    tools::consensus_pool_test_with_storage(
        cfg.clone(),
        None,
        None,
        async move |mut pool_controller,
                    mut protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver,
                    storage| {
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
                        ProtocolCommand::IntegratedBlock { block_id, .. } => {
                            let block = storage
                                .retrieve_block(&block_id)
                                .expect(&format!("Block id : {} not found in storage", block_id));
                            let stored_block = block.read();
                            Some((block_id, stored_block.block.clone()))
                        }
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
                        let mut eds: Vec<(EndorsementId, SignedEndorsement)> = Vec::new();
                        for (index, creator) in creators.iter().enumerate() {
                            let ed = if *creator == address_a {
                                create_endorsement(priv_a, target_slot, parent, index as u32)
                            } else if *creator == address_b {
                                create_endorsement(priv_b, target_slot, parent, index as u32)
                            } else {
                                panic!("invalid endorser choice");
                            };
                            eds.push((ed.content.compute_id().unwrap(), ed));
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
                                    .map(|op| {
                                        (op.content.compute_id().unwrap(), op.clone(), op_size)
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
            let (_block_id, block) = protocol_controller
                .wait_command(500.into(), |cmd| match cmd {
                    ProtocolCommand::IntegratedBlock { block_id, .. } => {
                        let block = storage
                            .retrieve_block(&block_id)
                            .expect(&format!("Block id : {} not found in storage", block_id));
                        let stored_block = block.read();
                        Some((block_id, stored_block.block.clone()))
                    }
                    _ => None,
                })
                .await
                .expect("timeout while waiting for block");

            // assert it's the expected block
            assert_eq!(block.header.content.slot, Slot::new(2, 0));

            // assert it has included the sc operation first
            match block.operations[0].content.op {
                OperationType::ExecuteSC { .. } => {}
                _ => panic!("unexpected operation included first"),
            }

            // assert it includes the sent endorsements
            assert_eq!(block.header.content.endorsements.len(), eds.len());
            for (e_found, (e_expected_id, e_expected)) in
                block.header.content.endorsements.iter().zip(eds.iter())
            {
                assert_eq!(e_found.content.compute_id().unwrap(), *e_expected_id);
                assert_eq!(e_expected.content.compute_id().unwrap(), *e_expected_id);
            }

            // create empty block
            let (_block_id, header) = Signed::new_signed(
                BlockHeader {
                    creator: block.header.content.creator,
                    slot: block.header.content.slot,
                    parents: block.header.content.parents.clone(),
                    operation_merkle_root: Hash::compute_from(&Vec::new()[..]),
                    endorsements: eds.iter().map(|(_e_id, endo)| endo.clone()).collect(),
                },
                &priv_a,
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
