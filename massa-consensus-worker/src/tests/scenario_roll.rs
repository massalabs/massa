// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_consensus_exports::tools;
use massa_consensus_exports::{settings::ConsensusChannels, ConsensusConfig};
use massa_execution_exports::test_exports::MockExecutionController;
use massa_models::{Address, Amount, BlockId, Slot};
use massa_pool::PoolCommand;
use massa_protocol_exports::ProtocolCommand;
use massa_time::MassaTime;
use num::rational::Ratio;
use rand::{prelude::SliceRandom, rngs::StdRng, SeedableRng};
use serial_test::serial;
use std::collections::HashMap;
use std::str::FromStr;

use crate::{
    start_consensus_controller,
    tests::{
        mock_pool_controller::MockPoolController,
        mock_protocol_controller::MockProtocolController,
        tools::{
            consensus_pool_test, create_block, create_block_with_operations, create_roll_buy,
            create_roll_sell, get_creator_for_draw, propagate_block, random_address_on_thread,
            wait_pool_slot,
        },
    },
};
use massa_models::ledger_models::LedgerData;
use massa_models::prehash::Set;

#[tokio::test]
#[serial]
async fn test_roll() {
    // setup logging
    /*
        stderrlog::new()
            .verbosity(2)
            .timestamp(stderrlog::Timestamp::Millisecond)
            .init()
            .unwrap();
    */
    let init_time: MassaTime = 1000.into();
    let mut cfg = ConsensusConfig {
        t0: 500.into(),
        periods_per_cycle: 2,
        delta_f0: 3,
        block_reward: Amount::default(),
        roll_price: Amount::from_str("1000").unwrap(),
        operation_validity_periods: 100,
        genesis_timestamp: MassaTime::now().unwrap().saturating_add(init_time),
        ..Default::default()
    };
    // define addresses use for the test
    // addresses 1 and 2 both in thread 0
    let (address_1, priv_1, _) = random_address_on_thread(0, cfg.thread_count).into();
    let (address_2, priv_2, _) = random_address_on_thread(0, cfg.thread_count).into();

    let mut ledger = HashMap::new();
    ledger.insert(
        address_2,
        LedgerData::new(Amount::from_str("10000").unwrap()),
    );
    let initial_ledger_file = tools::generate_ledger_file(&ledger);
    cfg.initial_ledger_path = initial_ledger_file.path().to_path_buf();

    let staking_keys_file = tools::generate_staking_keys_file(&[priv_2]);
    cfg.staking_keys_path = staking_keys_file.path().to_path_buf();

    let initial_rolls_file = tools::generate_default_roll_counts_file(vec![priv_1]);
    cfg.initial_rolls_path = initial_rolls_file.path().to_path_buf();

    consensus_pool_test(
        cfg.clone(),
        None,
        None,
        async move |mut pool_controller,
                    mut protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver| {
            let mut parents: Vec<BlockId> = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .best_parents
                .iter()
                .map(|(b, _p)| *b)
                .collect();

            // operations
            let rb_a1_r1_err = create_roll_buy(priv_1, 1, 90, 0);
            let rs_a2_r1_err = create_roll_sell(priv_2, 1, 90, 0);
            let rb_a2_r1 = create_roll_buy(priv_2, 1, 90, 0);
            let rs_a2_r1 = create_roll_sell(priv_2, 1, 90, 0);
            let rb_a2_r2 = create_roll_buy(priv_2, 2, 90, 0);
            let rs_a2_r2 = create_roll_sell(priv_2, 2, 90, 0);

            let mut addresses = Set::<Address>::default();
            addresses.insert(address_2);
            let addresses = addresses;

            // cycle 0
            let (_, block1_err1, _) = create_block_with_operations(
                &cfg,
                Slot::new(1, 0),
                &parents,
                priv_1,
                vec![rb_a1_r1_err],
            );
            tokio::time::sleep(init_time.to_duration()).await;
            wait_pool_slot(&mut pool_controller, cfg.t0, 1, 0).await;
            // invalid because a1 has not enough coins to buy a roll
            propagate_block(&mut protocol_controller, block1_err1, false, 150).await;

            let (_, block1_err2, _) = create_block_with_operations(
                &cfg,
                Slot::new(1, 0),
                &parents,
                priv_1,
                vec![rs_a2_r1_err],
            );
            // invalid because a2 does not have enough rolls to sell
            propagate_block(&mut protocol_controller, block1_err2, false, 150).await;

            let (id_1, block1, _) = create_block_with_operations(
                &cfg,
                Slot::new(1, 0),
                &parents,
                priv_1,
                vec![rb_a2_r1],
            );

            // valid
            propagate_block(&mut protocol_controller, block1, true, 150).await;
            parents[0] = id_1;

            let addr_state = consensus_command_sender
                .get_addresses_info(addresses.clone())
                .await
                .unwrap()
                .get(&address_2)
                .unwrap()
                .clone();
            assert_eq!(addr_state.rolls.active_rolls, 0);
            assert_eq!(addr_state.rolls.final_rolls, 0);
            assert_eq!(addr_state.rolls.candidate_rolls, 1);
            assert_eq!(
                addr_state.ledger_info.candidate_ledger_info.balance,
                Amount::from_str("9000").unwrap()
            );

            let (id_1t1, block1t1, _) =
                create_block_with_operations(&cfg, Slot::new(1, 1), &parents, priv_1, vec![]);

            wait_pool_slot(&mut pool_controller, cfg.t0, 1, 1).await;
            // valid
            propagate_block(&mut protocol_controller, block1t1, true, 150).await;
            parents[1] = id_1t1;

            // cycle 1

            let (id_2, block2, _) = create_block_with_operations(
                &cfg,
                Slot::new(2, 0),
                &parents,
                priv_1,
                vec![rs_a2_r1],
            );

            wait_pool_slot(&mut pool_controller, cfg.t0, 2, 0).await;
            // valid
            propagate_block(&mut protocol_controller, block2, true, 150).await;
            parents[0] = id_2;

            let addr_state = consensus_command_sender
                .get_addresses_info(addresses.clone())
                .await
                .unwrap()
                .get(&address_2)
                .unwrap()
                .clone();
            assert_eq!(addr_state.rolls.active_rolls, 0);
            assert_eq!(addr_state.rolls.final_rolls, 0);
            assert_eq!(addr_state.rolls.candidate_rolls, 0);
            let balance = addr_state.ledger_info.candidate_ledger_info.balance;
            assert_eq!(balance, Amount::from_str("9000").unwrap());

            let (id_2t, block2t2, _) =
                create_block_with_operations(&cfg, Slot::new(2, 1), &parents, priv_1, vec![]);
            wait_pool_slot(&mut pool_controller, cfg.t0, 2, 1).await;
            // valid
            propagate_block(&mut protocol_controller, block2t2, true, 150).await;
            parents[1] = id_2t;

            // miss block 3 in thread 0

            // block 3 in thread 1
            let (id_3t1, block3t1, _) =
                create_block_with_operations(&cfg, Slot::new(3, 1), &parents, priv_1, vec![]);
            wait_pool_slot(&mut pool_controller, cfg.t0, 3, 1).await;
            // valid
            propagate_block(&mut protocol_controller, block3t1, true, 150).await;
            parents[1] = id_3t1;

            // cycle 2

            // miss block 4

            let (id_4t1, block4t1, _) =
                create_block_with_operations(&cfg, Slot::new(4, 1), &parents, priv_1, vec![]);
            wait_pool_slot(&mut pool_controller, cfg.t0, 4, 1).await;
            // valid
            propagate_block(&mut protocol_controller, block4t1, true, 150).await;
            parents[1] = id_4t1;

            let (id_5, block5, _) =
                create_block_with_operations(&cfg, Slot::new(5, 0), &parents, priv_1, vec![]);
            wait_pool_slot(&mut pool_controller, cfg.t0, 5, 0).await;
            // valid
            propagate_block(&mut protocol_controller, block5, true, 150).await;
            parents[0] = id_5;

            let (id_5t1, block5t1, _) =
                create_block_with_operations(&cfg, Slot::new(5, 1), &parents, priv_1, vec![]);
            wait_pool_slot(&mut pool_controller, cfg.t0, 5, 1).await;
            // valid
            propagate_block(&mut protocol_controller, block5t1, true, 150).await;
            parents[1] = id_5t1;

            // cycle 3
            let draws: HashMap<_, _> = consensus_command_sender
                .get_selection_draws(Slot::new(6, 0), Slot::new(8, 0))
                .await
                .unwrap()
                .into_iter()
                .map(|(s, (b, _e))| (s, b))
                .collect();

            let other_addr = if *draws.get(&Slot::new(6, 0)).unwrap() == address_1 {
                address_2
            } else {
                address_1
            };

            let (_, block6_err, _) = create_block_with_operations(
                &cfg,
                Slot::new(6, 0),
                &parents,
                get_creator_for_draw(&other_addr, &vec![priv_1, priv_2]),
                vec![],
            );
            wait_pool_slot(&mut pool_controller, cfg.t0, 6, 0).await;
            // invalid: other_addr wasn't drawn for that block creation
            propagate_block(&mut protocol_controller, block6_err, false, 150).await;

            let (id_6, block6, _) = create_block_with_operations(
                &cfg,
                Slot::new(6, 0),
                &parents,
                get_creator_for_draw(draws.get(&Slot::new(6, 0)).unwrap(), &vec![priv_1, priv_2]),
                vec![],
            );

            // valid
            propagate_block(&mut protocol_controller, block6, true, 150).await;
            parents[0] = id_6;

            let addr_state = consensus_command_sender
                .get_addresses_info(addresses.clone())
                .await
                .unwrap()
                .get(&address_2)
                .unwrap()
                .clone();
            assert_eq!(addr_state.rolls.active_rolls, 1);
            assert_eq!(addr_state.rolls.final_rolls, 0);
            assert_eq!(addr_state.rolls.candidate_rolls, 0);

            let (id_6t1, block6t1, _) = create_block_with_operations(
                &cfg,
                Slot::new(6, 1),
                &parents,
                get_creator_for_draw(draws.get(&Slot::new(6, 1)).unwrap(), &vec![priv_1, priv_2]),
                vec![],
            );

            wait_pool_slot(&mut pool_controller, cfg.t0, 6, 1).await;
            // valid
            propagate_block(&mut protocol_controller, block6t1, true, 150).await;
            parents[1] = id_6t1;

            let (id_7, block7, _) = create_block_with_operations(
                &cfg,
                Slot::new(7, 0),
                &parents,
                get_creator_for_draw(draws.get(&Slot::new(7, 0)).unwrap(), &vec![priv_1, priv_2]),
                vec![],
            );

            wait_pool_slot(&mut pool_controller, cfg.t0, 7, 0).await;
            // valid
            propagate_block(&mut protocol_controller, block7, true, 150).await;
            parents[0] = id_7;

            let addr_state = consensus_command_sender
                .get_addresses_info(addresses.clone())
                .await
                .unwrap()
                .get(&address_2)
                .unwrap()
                .clone();
            assert_eq!(addr_state.rolls.active_rolls, 1);
            assert_eq!(addr_state.rolls.final_rolls, 0);
            assert_eq!(addr_state.rolls.candidate_rolls, 0);

            let (id_7t1, block7t1, _) = create_block_with_operations(
                &cfg,
                Slot::new(7, 1),
                &parents,
                get_creator_for_draw(draws.get(&Slot::new(7, 1)).unwrap(), &vec![priv_1, priv_2]),
                vec![],
            );

            wait_pool_slot(&mut pool_controller, cfg.t0, 7, 1).await;
            // valid
            propagate_block(&mut protocol_controller, block7t1, true, 150).await;
            parents[1] = id_7t1;

            // cycle 4

            let (id_8, block8, _) = create_block_with_operations(
                &cfg,
                Slot::new(8, 0),
                &parents,
                priv_1,
                vec![rb_a2_r2],
            );
            wait_pool_slot(&mut pool_controller, cfg.t0, 8, 0).await;
            // valid
            propagate_block(&mut protocol_controller, block8, true, 150).await;
            parents[0] = id_8;

            let addr_state = consensus_command_sender
                .get_addresses_info(addresses.clone())
                .await
                .unwrap()
                .get(&address_2)
                .unwrap()
                .clone();
            assert_eq!(addr_state.rolls.active_rolls, 0);
            assert_eq!(addr_state.rolls.final_rolls, 0);
            assert_eq!(addr_state.rolls.candidate_rolls, 2);
            let balance = addr_state.ledger_info.candidate_ledger_info.balance;
            assert_eq!(balance, Amount::from_str("7000").unwrap());

            let (id_8t1, block8t1, _) =
                create_block_with_operations(&cfg, Slot::new(8, 1), &parents, priv_1, vec![]);
            wait_pool_slot(&mut pool_controller, cfg.t0, 8, 1).await;
            // valid
            propagate_block(&mut protocol_controller, block8t1, true, 150).await;
            parents[1] = id_8t1;

            let (id_9, block9, _) = create_block_with_operations(
                &cfg,
                Slot::new(9, 0),
                &parents,
                priv_1,
                vec![rs_a2_r2],
            );
            wait_pool_slot(&mut pool_controller, cfg.t0, 9, 0).await;
            // valid
            propagate_block(&mut protocol_controller, block9, true, 150).await;
            parents[0] = id_9;

            let addr_state = consensus_command_sender
                .get_addresses_info(addresses.clone())
                .await
                .unwrap()
                .get(&address_2)
                .unwrap()
                .clone();
            assert_eq!(addr_state.rolls.active_rolls, 0);
            assert_eq!(addr_state.rolls.final_rolls, 0);
            assert_eq!(addr_state.rolls.candidate_rolls, 0);
            let balance = addr_state.ledger_info.candidate_ledger_info.balance;
            assert_eq!(balance, Amount::from_str("9000").unwrap());

            let (id_9t1, block9t1, _) =
                create_block_with_operations(&cfg, Slot::new(9, 1), &parents, priv_1, vec![]);
            wait_pool_slot(&mut pool_controller, cfg.t0, 9, 1).await;
            // valid
            propagate_block(&mut protocol_controller, block9t1, true, 150).await;
            parents[1] = id_9t1;

            // cycle 5

            let (id_10, block10, _) =
                create_block_with_operations(&cfg, Slot::new(10, 0), &parents, priv_1, vec![]);
            wait_pool_slot(&mut pool_controller, cfg.t0, 10, 0).await;
            // valid
            propagate_block(&mut protocol_controller, block10, true, 150).await;
            parents[0] = id_10;

            let addr_state = consensus_command_sender
                .get_addresses_info(addresses.clone())
                .await
                .unwrap()
                .get(&address_2)
                .unwrap()
                .clone();
            assert_eq!(addr_state.rolls.active_rolls, 0);
            assert_eq!(addr_state.rolls.final_rolls, 2);
            assert_eq!(addr_state.rolls.candidate_rolls, 0);

            let balance = consensus_command_sender
                .get_addresses_info(addresses.clone())
                .await
                .unwrap()
                .get(&address_2)
                .unwrap()
                .ledger_info
                .candidate_ledger_info
                .balance;
            assert_eq!(balance, Amount::from_str("10000").unwrap());

            let (id_10t1, block10t1, _) =
                create_block_with_operations(&cfg, Slot::new(10, 1), &parents, priv_1, vec![]);
            wait_pool_slot(&mut pool_controller, cfg.t0, 10, 1).await;
            // valid
            propagate_block(&mut protocol_controller, block10t1, true, 150).await;
            parents[1] = id_10t1;

            let (id_11, block11, _) =
                create_block_with_operations(&cfg, Slot::new(11, 0), &parents, priv_1, vec![]);
            wait_pool_slot(&mut pool_controller, cfg.t0, 11, 0).await;
            // valid
            propagate_block(&mut protocol_controller, block11, true, 150).await;
            parents[0] = id_11;

            let addr_state = consensus_command_sender
                .get_addresses_info(addresses.clone())
                .await
                .unwrap()
                .get(&address_2)
                .unwrap()
                .clone();
            assert_eq!(addr_state.rolls.active_rolls, 0);
            assert_eq!(addr_state.rolls.final_rolls, 0);
            assert_eq!(addr_state.rolls.candidate_rolls, 0);
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

#[ignore]
#[tokio::test]
#[serial]
async fn test_roll_block_creation() {
    // setup logging
    /*
    stderrlog::new()
        .verbosity(4)
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()
        .unwrap();
    */

    let mut cfg = ConsensusConfig {
        block_reward: Amount::default(),
        delta_f0: 3,
        disable_block_creation: false,
        operation_validity_periods: 10,
        max_block_size: 500,
        max_operations_per_block: 5000,
        periods_per_cycle: 2,
        roll_price: Amount::from_str("1000").unwrap(),
        t0: 500.into(),
        ..Default::default()
    };
    // define addresses use for the test
    // addresses 1 and 2 both in thread 0
    let (_, priv_1, _) = random_address_on_thread(0, cfg.thread_count).into();
    let (address_2, priv_2, _) = random_address_on_thread(0, cfg.thread_count).into();

    let mut ledger = HashMap::new();
    ledger.insert(
        address_2,
        LedgerData::new(Amount::from_str("10000").unwrap()),
    );
    let initial_ledger_file = tools::generate_ledger_file(&ledger);
    let staking_keys_file = tools::generate_staking_keys_file(&[priv_1]);
    let initial_rolls_file = tools::generate_default_roll_counts_file(vec![priv_1]);
    cfg.initial_ledger_path = initial_ledger_file.path().to_path_buf();
    cfg.staking_keys_path = staking_keys_file.path().to_path_buf();
    cfg.initial_rolls_path = initial_rolls_file.path().to_path_buf();
    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let (mut pool_controller, pool_command_sender) = MockPoolController::new();
    let (execution_controller, _execution_rx) = MockExecutionController::new_with_receiver();

    let init_time: MassaTime = 1000.into();
    cfg.genesis_timestamp = MassaTime::now().unwrap().saturating_add(init_time);

    // launch consensus controller
    let (consensus_command_sender, _consensus_event_receiver, _consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            ConsensusChannels {
                execution_controller,
                protocol_command_sender: protocol_command_sender.clone(),
                protocol_event_receiver,
                pool_command_sender,
            },
            None,
            None,
            0,
        )
        .await
        .expect("could not start consensus controller");

    // operations
    let rb_a2_r1 = create_roll_buy(priv_2, 1, 90, 0);
    let rs_a2_r1 = create_roll_sell(priv_2, 1, 90, 0);

    let mut addresses = Set::<Address>::default();
    addresses.insert(address_2);
    let addresses = addresses;

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

    // cycle 0

    // respond to first pool batch command
    pool_controller
        .wait_command(300.into(), |cmd| match cmd {
            PoolCommand::GetOperationBatch {
                response_tx,
                target_slot,
                ..
            } => {
                assert_eq!(target_slot, Slot::new(1, 0));
                response_tx
                    .send(vec![(
                        rb_a2_r1.clone().get_operation_id().unwrap(),
                        rb_a2_r1.clone(),
                        10,
                    )])
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
    assert_eq!(block.header.content.slot, Slot::new(1, 0));
    assert_eq!(block.operations.len(), 1);
    assert_eq!(
        block.operations[0].get_operation_id().unwrap(),
        rb_a2_r1.clone().get_operation_id().unwrap()
    );

    let addr_state = consensus_command_sender
        .get_addresses_info(addresses.clone())
        .await
        .unwrap()
        .get(&address_2)
        .unwrap()
        .clone();
    assert_eq!(addr_state.rolls.active_rolls, 0);
    assert_eq!(addr_state.rolls.final_rolls, 0);
    assert_eq!(addr_state.rolls.candidate_rolls, 1);

    let balance = consensus_command_sender
        .get_addresses_info(addresses.clone())
        .await
        .unwrap()
        .get(&address_2)
        .unwrap()
        .ledger_info
        .candidate_ledger_info
        .balance;
    assert_eq!(balance, Amount::from_str("9000").unwrap());

    wait_pool_slot(&mut pool_controller, cfg.t0, 1, 1).await;
    // slot 1,1
    pool_controller
        .wait_command(300.into(), |cmd| match cmd {
            PoolCommand::GetOperationBatch {
                response_tx,
                target_slot,
                ..
            } => {
                assert_eq!(target_slot, Slot::new(1, 1));
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
        .expect("timeout while waiting for operation batch request");

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
    assert_eq!(block.header.content.slot, Slot::new(1, 1));
    assert!(block.operations.is_empty());

    // cycle 1

    pool_controller
        .wait_command(300.into(), |cmd| match cmd {
            PoolCommand::GetOperationBatch {
                response_tx,
                target_slot,
                ..
            } => {
                assert_eq!(target_slot, Slot::new(2, 0));
                response_tx
                    .send(vec![(
                        rs_a2_r1.clone().get_operation_id().unwrap(),
                        rs_a2_r1.clone(),
                        10,
                    )])
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
    assert_eq!(block.operations.len(), 1);
    assert_eq!(
        block.operations[0].get_operation_id().unwrap(),
        rs_a2_r1.clone().get_operation_id().unwrap()
    );

    let addr_state = consensus_command_sender
        .get_addresses_info(addresses.clone())
        .await
        .unwrap()
        .get(&address_2)
        .unwrap()
        .clone();
    assert_eq!(addr_state.rolls.active_rolls, 0);
    assert_eq!(addr_state.rolls.final_rolls, 0);
    assert_eq!(addr_state.rolls.candidate_rolls, 0);
    let balance = addr_state.ledger_info.candidate_ledger_info.balance;
    assert_eq!(balance, Amount::from_str("9000").unwrap());
}

#[tokio::test]
#[serial]
async fn test_roll_deactivation() {
    /*
        Scenario:
            * deactivation threshold at 50%
            * thread_count = 10
            * lookback_cycles = 2
            * periods_per_cycle = 10
            * delta_f0 = 2
            * all addresses have 1 roll initially
            * in cycle 0:
                * an address A0 in thread 0 produces 20% of its blocks
                * an address B0 in thread 0 produces 80% of its blocks
                * an address A1 in thread 1 produces 20% of its blocks
                * an address B1 in thread 1 produces 80% of its blocks
            * at the next cycles, all addresses produce all their blocks
            * at the 1st block of thread 0 in cycle 2:
              * address A0 has (0 candidate, 1 final, 1 active) rolls
              * address B0 has (1 candidate, 1 final, 1 active) rolls
              * address A1 has (1 candidate, 1 final, 1 active) rolls
              * address B1 has (1 candidate, 1 final, 1 active) rolls
            * at the 1st block of thread 1 in cycle 2:
              * address A0 has (0 candidate, 1 final, 1 active) rolls
              * address B0 has (1 candidate, 1 final, 1 active) rolls
              * address A1 has (0 candidate, 1 final, 1 active) rolls
              * address B1 has (1 candidate, 1 final, 1 active) rolls
    */

    let mut cfg = ConsensusConfig {
        delta_f0: 2,
        thread_count: 4,
        periods_per_cycle: 5,
        pos_lookback_cycles: 1,
        t0: 400.into(),
        operation_batch_size: 500,
        roll_price: Amount::from_raw(10),
        pos_miss_rate_deactivation_threshold: Ratio::new(50, 100),
        ..Default::default()
    };

    // setup addresses
    let (address_a0, privkey_a0, _) = random_address_on_thread(0, cfg.thread_count).into();
    let (address_b0, privkey_b0, _) = random_address_on_thread(0, cfg.thread_count).into();
    let (address_a1, privkey_a1, _) = random_address_on_thread(1, cfg.thread_count).into();
    let (address_b1, privkey_b1, _) = random_address_on_thread(1, cfg.thread_count).into();

    let initial_ledger_file = tools::generate_ledger_file(&HashMap::new());
    let staking_keys_file = tools::generate_staking_keys_file(&[]);
    let initial_rolls_file = tools::generate_default_roll_counts_file(vec![
        privkey_a0, privkey_a1, privkey_b0, privkey_b1,
    ]);

    cfg.initial_ledger_path = initial_ledger_file.path().to_path_buf();
    cfg.staking_keys_path = staking_keys_file.path().to_path_buf();
    cfg.initial_rolls_path = initial_rolls_file.path().to_path_buf();

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let (mut pool_controller, pool_command_sender) = MockPoolController::new();
    let (execution_controller, _execution_rx) = MockExecutionController::new_with_receiver();

    cfg.genesis_timestamp = MassaTime::now().unwrap().saturating_add(300.into());
    // launch consensus controller
    let (consensus_command_sender, _consensus_event_receiver, _consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            ConsensusChannels {
                execution_controller,
                protocol_command_sender: protocol_command_sender.clone(),
                protocol_event_receiver,
                pool_command_sender,
            },
            None,
            None,
            0,
        )
        .await
        .expect("could not start consensus controller");

    let mut cur_slot = Slot::new(0, 0);
    let mut best_parents = consensus_command_sender
        .get_block_graph_status(None, None)
        .await
        .unwrap()
        .genesis_blocks;
    let mut cycle_draws = HashMap::new();
    let mut draws_cycle = None;
    'outer: loop {
        // wait for slot info
        let latest_slot = pool_controller
            .wait_command(cfg.t0.checked_mul(2).unwrap(), |cmd| match cmd {
                PoolCommand::UpdateCurrentSlot(s) => Some(s),
                _ => None,
            })
            .await
            .expect("timeout while waiting for slot");
        // apply all slots in-between
        while cur_slot <= latest_slot {
            // skip genesis
            if cur_slot.period == 0 {
                cur_slot = cur_slot.get_next_slot(cfg.thread_count).unwrap();
                continue;
            }
            let cur_cycle = cur_slot.get_cycle(cfg.periods_per_cycle);

            // get draws
            if draws_cycle != Some(cur_cycle) {
                cycle_draws = consensus_command_sender
                    .get_selection_draws(
                        Slot::new(std::cmp::max(cur_cycle * cfg.periods_per_cycle, 1), 0),
                        Slot::new((cur_cycle + 1) * cfg.periods_per_cycle, 0),
                    )
                    .await
                    .unwrap()
                    .into_iter()
                    .map(|(k, (v, _e))| (k, Some(v)))
                    .collect::<HashMap<Slot, Option<Address>>>();
                if cur_cycle == 0 {
                    // controlled misses in cycle 0
                    for address in [address_a0, address_a1, address_b0, address_b1] {
                        let mut address_draws: Vec<Slot> = cycle_draws
                            .iter()
                            .filter_map(|(s, opt_a)| {
                                if let Some(a) = opt_a {
                                    if *a == address {
                                        return Some(*s);
                                    }
                                }
                                None
                            })
                            .collect();
                        assert!(
                            !address_draws.is_empty(),
                            "unlucky seed: address has no draws in cycle 0, cannot perform test"
                        );
                        address_draws.shuffle(&mut StdRng::from_entropy());
                        let produce_count: usize = if address == address_a0 || address == address_a1
                        {
                            // produce less than 20%
                            20 * address_draws.len() / 100
                        } else {
                            // produce more than 80%
                            std::cmp::min(address_draws.len(), (80 * address_draws.len() / 100) + 1)
                        };
                        address_draws.truncate(produce_count);
                        for (slt, opt_addr) in cycle_draws.iter_mut() {
                            if *opt_addr == Some(address) && !address_draws.contains(slt) {
                                *opt_addr = None;
                            }
                        }
                    }
                }
                draws_cycle = Some(cur_cycle);
            }
            let cur_draw = cycle_draws[&cur_slot];

            // create and propagate block
            if let Some(addr) = cur_draw {
                let creator_privkey = if addr == address_a0 {
                    privkey_a0
                } else if addr == address_a1 {
                    privkey_a1
                } else if addr == address_b0 {
                    privkey_b0
                } else if addr == address_b1 {
                    privkey_b1
                } else {
                    panic!("invalid address selected");
                };
                let block_id = propagate_block(
                    &mut protocol_controller,
                    create_block(&cfg, cur_slot, best_parents.clone(), creator_privkey).1,
                    true,
                    500,
                )
                .await;

                // update best parents
                best_parents[cur_slot.thread as usize] = block_id;
            }

            // check candidate rolls
            let addrs_info = consensus_command_sender
                .get_addresses_info(
                    vec![address_a0, address_a1, address_b0, address_b1]
                        .into_iter()
                        .collect(),
                )
                .await
                .unwrap()
                .clone();
            if cur_slot.period == (1 + cfg.pos_lookback_cycles) * cfg.periods_per_cycle {
                if cur_slot.thread == 0 {
                    assert_eq!(addrs_info[&address_a0].rolls.candidate_rolls, 0);
                    assert_eq!(addrs_info[&address_b0].rolls.candidate_rolls, 1);
                    assert_eq!(addrs_info[&address_a1].rolls.candidate_rolls, 1);
                    assert_eq!(addrs_info[&address_b1].rolls.candidate_rolls, 1);
                } else if cur_slot.thread == 1 {
                    assert_eq!(addrs_info[&address_a0].rolls.candidate_rolls, 0);
                    assert_eq!(addrs_info[&address_b0].rolls.candidate_rolls, 1);
                    assert_eq!(addrs_info[&address_a1].rolls.candidate_rolls, 0);
                    assert_eq!(addrs_info[&address_b1].rolls.candidate_rolls, 1);
                } else {
                    break 'outer;
                }
            } else {
                assert_eq!(addrs_info[&address_a0].rolls.candidate_rolls, 1);
                assert_eq!(addrs_info[&address_b0].rolls.candidate_rolls, 1);
                assert_eq!(addrs_info[&address_a1].rolls.candidate_rolls, 1);
                assert_eq!(addrs_info[&address_b1].rolls.candidate_rolls, 1);
            }

            cur_slot = cur_slot.get_next_slot(cfg.thread_count).unwrap();
        }
    }
}
