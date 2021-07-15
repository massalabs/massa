// Copyright (c) 2021 MASSA LABS <info@massa.net>

use std::collections::{HashMap, HashSet};

use communication::protocol::ProtocolCommand;
use models::{Address, Slot};
use pool::PoolCommand;
use serial_test::serial;
use time::UTime;

use crate::{
    start_consensus_controller,
    tests::{
        mock_pool_controller::MockPoolController,
        mock_protocol_controller::MockProtocolController,
        tools::{
            self, create_block_with_operations, create_roll_buy, create_roll_sell,
            generate_ledger_file, get_creator_for_draw, propagate_block, wait_pool_slot,
        },
    },
    LedgerData,
};

#[tokio::test]
#[serial]
async fn test_roll() {
    // setup logging
    /*
    stderrlog::new()
        .verbosity(4)
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()
        .unwrap();
    */
    let thread_count = 2;
    // define addresses use for the test
    // addresses 1 and 2 both in thread 0
    let mut priv_1 = crypto::generate_random_private_key();
    let mut pubkey_1 = crypto::derive_public_key(&priv_1);
    let mut address_1 = Address::from_public_key(&pubkey_1).unwrap();
    while 0 != address_1.get_thread(thread_count) {
        priv_1 = crypto::generate_random_private_key();
        pubkey_1 = crypto::derive_public_key(&priv_1);
        address_1 = Address::from_public_key(&pubkey_1).unwrap();
    }
    assert_eq!(0, address_1.get_thread(thread_count));

    let mut priv_2 = crypto::generate_random_private_key();
    let mut pubkey_2 = crypto::derive_public_key(&priv_2);
    let mut address_2 = Address::from_public_key(&pubkey_2).unwrap();
    while 0 != address_2.get_thread(thread_count) {
        priv_2 = crypto::generate_random_private_key();
        pubkey_2 = crypto::derive_public_key(&priv_2);
        address_2 = Address::from_public_key(&pubkey_2).unwrap();
    }
    assert_eq!(0, address_2.get_thread(thread_count));

    let mut ledger = HashMap::new();
    ledger.insert(address_2, LedgerData { balance: 10_000 });
    let ledger_file = generate_ledger_file(&ledger);

    let staking_file = tools::generate_staking_keys_file(&vec![priv_2]);
    let roll_counts_file = tools::generate_default_roll_counts_file(vec![priv_1]);
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.periods_per_cycle = 2;
    cfg.pos_lookback_cycles = 2;
    cfg.pos_lock_cycles = 1;
    cfg.t0 = 500.into();
    cfg.delta_f0 = 3;
    cfg.disable_block_creation = true;
    cfg.thread_count = thread_count;
    cfg.block_reward = 0;
    cfg.roll_price = 1000;
    cfg.operation_validity_periods = 100;
    cfg.genesis_timestamp = UTime::now(0).unwrap().saturating_add(300.into());

    tools::consensus_pool_test(
        cfg.clone(),
        None,
        None,
        None,
        async move |mut pool_controller,
                    mut protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver| {
            let mut parents = consensus_command_sender
                .get_block_graph_status()
                .await
                .expect("could not get block graph status")
                .best_parents;

            // operations
            let rb_a1_r1_err = create_roll_buy(priv_1, 1, 90, 0);
            let rs_a2_r1_err = create_roll_sell(priv_2, 1, 90, 0);
            let rb_a2_r1 = create_roll_buy(priv_2, 1, 90, 0);
            let rs_a2_r1 = create_roll_sell(priv_2, 1, 90, 0);
            let rb_a2_r2 = create_roll_buy(priv_2, 2, 90, 0);
            let rs_a2_r2 = create_roll_sell(priv_2, 2, 90, 0);

            let mut addresses = HashSet::new();
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
            wait_pool_slot(&mut &mut pool_controller, cfg.t0, 1, 0).await;
            propagate_block(&mut protocol_controller, block1_err1, false, 150).await;

            let (_, block1_err2, _) = create_block_with_operations(
                &cfg,
                Slot::new(1, 0),
                &parents,
                priv_1,
                vec![rs_a2_r1_err],
            );
            propagate_block(&mut protocol_controller, block1_err2, false, 150).await;

            let (id_1, block1, _) = create_block_with_operations(
                &cfg,
                Slot::new(1, 0),
                &parents,
                priv_1,
                vec![rb_a2_r1],
            );

            propagate_block(&mut protocol_controller, block1, true, 150).await;
            parents[0] = id_1;

            let addr_state = consensus_command_sender
                .get_addresses_info(addresses.clone())
                .await
                .unwrap()
                .get(&address_2)
                .unwrap()
                .clone();
            assert_eq!(addr_state.active_rolls, Some(0));
            assert_eq!(addr_state.final_rolls, 0);
            assert_eq!(addr_state.candidate_rolls, 1);
            assert_eq!(addr_state.candidate_ledger_data.balance, 9000);

            let (id_1t1, block1t1, _) =
                create_block_with_operations(&cfg, Slot::new(1, 1), &parents, priv_1, vec![]);

            wait_pool_slot(&mut &mut pool_controller, cfg.t0, 1, 1).await;
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

            wait_pool_slot(&mut &mut pool_controller, cfg.t0, 2, 0).await;
            propagate_block(&mut protocol_controller, block2, true, 150).await;
            parents[0] = id_2;

            let addr_state = consensus_command_sender
                .get_addresses_info(addresses.clone())
                .await
                .unwrap()
                .get(&address_2)
                .unwrap()
                .clone();
            assert_eq!(addr_state.active_rolls, Some(0));
            assert_eq!(addr_state.final_rolls, 0);
            assert_eq!(addr_state.candidate_rolls, 0);
            let balance = addr_state.candidate_ledger_data.balance;
            assert_eq!(balance, 9000);

            let (id_2t, block2t2, _) =
                create_block_with_operations(&cfg, Slot::new(2, 1), &parents, priv_1, vec![]);
            wait_pool_slot(&mut &mut pool_controller, cfg.t0, 2, 1).await;
            propagate_block(&mut protocol_controller, block2t2, true, 150).await;
            parents[1] = id_2t;

            // miss block 3 in thread 0

            // block 3 in thread 1
            let (id_3t1, block3t1, _) =
                create_block_with_operations(&cfg, Slot::new(3, 1), &parents, priv_1, vec![]);
            wait_pool_slot(&mut &mut pool_controller, cfg.t0, 3, 1).await;
            propagate_block(&mut protocol_controller, block3t1, true, 150).await;
            parents[1] = id_3t1;

            // cycle 2

            //miss block 4

            let (id_4t1, block4t1, _) =
                create_block_with_operations(&cfg, Slot::new(4, 1), &parents, priv_1, vec![]);
            wait_pool_slot(&mut &mut pool_controller, cfg.t0, 4, 1).await;
            propagate_block(&mut protocol_controller, block4t1, true, 150).await;
            parents[1] = id_4t1;

            let (id_5, block5, _) =
                create_block_with_operations(&cfg, Slot::new(5, 0), &parents, priv_1, vec![]);
            wait_pool_slot(&mut &mut pool_controller, cfg.t0, 5, 0).await;
            propagate_block(&mut protocol_controller, block5, true, 150).await;
            parents[0] = id_5;

            let (id_5t1, block5t1, _) =
                create_block_with_operations(&cfg, Slot::new(5, 1), &parents, priv_1, vec![]);
            wait_pool_slot(&mut &mut pool_controller, cfg.t0, 5, 1).await;
            propagate_block(&mut protocol_controller, block5t1, true, 150).await;
            parents[1] = id_5t1;

            // cycle 3
            let draws: HashMap<_, _> = consensus_command_sender
                .get_selection_draws(Slot::new(6, 0), Slot::new(8, 0))
                .await
                .unwrap()
                .into_iter()
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
            wait_pool_slot(&mut &mut pool_controller, cfg.t0, 6, 0).await;
            propagate_block(&mut protocol_controller, block6_err, false, 150).await;

            let (id_6, block6, _) = create_block_with_operations(
                &cfg,
                Slot::new(6, 0),
                &parents,
                get_creator_for_draw(draws.get(&Slot::new(6, 0)).unwrap(), &vec![priv_1, priv_2]),
                vec![],
            );

            propagate_block(&mut protocol_controller, block6, true, 150).await;
            parents[0] = id_6;

            let addr_state = consensus_command_sender
                .get_addresses_info(addresses.clone())
                .await
                .unwrap()
                .get(&address_2)
                .unwrap()
                .clone();
            assert_eq!(addr_state.active_rolls, Some(1));
            assert_eq!(addr_state.final_rolls, 0);
            assert_eq!(addr_state.candidate_rolls, 0);

            let (id_6t1, block6t1, _) = create_block_with_operations(
                &cfg,
                Slot::new(6, 1),
                &parents,
                get_creator_for_draw(draws.get(&Slot::new(6, 1)).unwrap(), &vec![priv_1, priv_2]),
                vec![],
            );

            wait_pool_slot(&mut &mut pool_controller, cfg.t0, 6, 1).await;
            propagate_block(&mut protocol_controller, block6t1, true, 150).await;
            parents[1] = id_6t1;

            let (id_7, block7, _) = create_block_with_operations(
                &cfg,
                Slot::new(7, 0),
                &parents,
                get_creator_for_draw(draws.get(&Slot::new(7, 0)).unwrap(), &vec![priv_1, priv_2]),
                vec![],
            );

            wait_pool_slot(&mut &mut pool_controller, cfg.t0, 7, 0).await;
            propagate_block(&mut protocol_controller, block7, true, 150).await;
            parents[0] = id_7;

            let addr_state = consensus_command_sender
                .get_addresses_info(addresses.clone())
                .await
                .unwrap()
                .get(&address_2)
                .unwrap()
                .clone();
            assert_eq!(addr_state.active_rolls, Some(1));
            assert_eq!(addr_state.final_rolls, 0);
            assert_eq!(addr_state.candidate_rolls, 0);

            let (id_7t1, block7t1, _) = create_block_with_operations(
                &cfg,
                Slot::new(7, 1),
                &parents,
                get_creator_for_draw(draws.get(&Slot::new(7, 1)).unwrap(), &vec![priv_1, priv_2]),
                vec![],
            );

            wait_pool_slot(&mut &mut pool_controller, cfg.t0, 7, 1).await;
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
            wait_pool_slot(&mut &mut pool_controller, cfg.t0, 8, 0).await;
            propagate_block(&mut protocol_controller, block8, true, 150).await;
            parents[0] = id_8;

            let addr_state = consensus_command_sender
                .get_addresses_info(addresses.clone())
                .await
                .unwrap()
                .get(&address_2)
                .unwrap()
                .clone();
            assert_eq!(addr_state.active_rolls, Some(0));
            assert_eq!(addr_state.final_rolls, 0);
            assert_eq!(addr_state.candidate_rolls, 2);
            let balance = addr_state.candidate_ledger_data.balance;
            assert_eq!(balance, 7000);

            let (id_8t1, block8t1, _) =
                create_block_with_operations(&cfg, Slot::new(8, 1), &parents, priv_1, vec![]);
            wait_pool_slot(&mut &mut pool_controller, cfg.t0, 8, 1).await;
            propagate_block(&mut protocol_controller, block8t1, true, 150).await;
            parents[1] = id_8t1;

            let (id_9, block9, _) = create_block_with_operations(
                &cfg,
                Slot::new(9, 0),
                &parents,
                priv_1,
                vec![rs_a2_r2],
            );
            wait_pool_slot(&mut &mut pool_controller, cfg.t0, 9, 0).await;
            propagate_block(&mut protocol_controller, block9, true, 150).await;
            parents[0] = id_9;

            let addr_state = consensus_command_sender
                .get_addresses_info(addresses.clone())
                .await
                .unwrap()
                .get(&address_2)
                .unwrap()
                .clone();
            assert_eq!(addr_state.active_rolls, Some(0));
            assert_eq!(addr_state.final_rolls, 0);
            assert_eq!(addr_state.candidate_rolls, 0);
            let balance = addr_state.candidate_ledger_data.balance;
            assert_eq!(balance, 9000);

            let (id_9t1, block9t1, _) =
                create_block_with_operations(&cfg, Slot::new(9, 1), &parents, priv_1, vec![]);
            wait_pool_slot(&mut &mut pool_controller, cfg.t0, 9, 1).await;
            propagate_block(&mut protocol_controller, block9t1, true, 150).await;
            parents[1] = id_9t1;

            // cycle 5

            let (id_10, block10, _) =
                create_block_with_operations(&cfg, Slot::new(10, 0), &parents, priv_1, vec![]);
            wait_pool_slot(&mut &mut pool_controller, cfg.t0, 10, 0).await;
            propagate_block(&mut protocol_controller, block10, true, 150).await;
            parents[0] = id_10;

            let addr_state = consensus_command_sender
                .get_addresses_info(addresses.clone())
                .await
                .unwrap()
                .get(&address_2)
                .unwrap()
                .clone();
            assert_eq!(addr_state.active_rolls, Some(0));
            assert_eq!(addr_state.final_rolls, 2);
            assert_eq!(addr_state.candidate_rolls, 0);

            let balance = consensus_command_sender
                .get_addresses_info(addresses.clone())
                .await
                .unwrap()
                .get(&address_2)
                .unwrap()
                .candidate_ledger_data
                .balance;
            assert_eq!(balance, 10000);

            let (id_10t1, block10t1, _) =
                create_block_with_operations(&cfg, Slot::new(10, 1), &parents, priv_1, vec![]);
            wait_pool_slot(&mut &mut pool_controller, cfg.t0, 10, 1).await;
            propagate_block(&mut protocol_controller, block10t1, true, 150).await;
            parents[1] = id_10t1;

            let (id_11, block11, _) =
                create_block_with_operations(&cfg, Slot::new(11, 0), &parents, priv_1, vec![]);
            wait_pool_slot(&mut &mut pool_controller, cfg.t0, 11, 0).await;
            propagate_block(&mut protocol_controller, block11, true, 150).await;
            parents[0] = id_11;

            let addr_state = consensus_command_sender
                .get_addresses_info(addresses.clone())
                .await
                .unwrap()
                .get(&address_2)
                .unwrap()
                .clone();
            assert_eq!(addr_state.active_rolls, Some(0));
            assert_eq!(addr_state.final_rolls, 0);
            assert_eq!(addr_state.candidate_rolls, 0);
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
async fn test_roll_block_creation() {
    // setup logging
    /*
    stderrlog::new()
        .verbosity(4)
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()
        .unwrap();
    */
    let thread_count = 2;
    // define addresses use for the test
    // addresses 1 and 2 both in thread 0
    let mut priv_1 = crypto::generate_random_private_key();
    let mut pubkey_1 = crypto::derive_public_key(&priv_1);
    let mut address_1 = Address::from_public_key(&pubkey_1).unwrap();
    while 0 != address_1.get_thread(thread_count) {
        priv_1 = crypto::generate_random_private_key();
        pubkey_1 = crypto::derive_public_key(&priv_1);
        address_1 = Address::from_public_key(&pubkey_1).unwrap();
    }
    assert_eq!(0, address_1.get_thread(thread_count));

    let mut priv_2 = crypto::generate_random_private_key();
    let mut pubkey_2 = crypto::derive_public_key(&priv_2);
    let mut address_2 = Address::from_public_key(&pubkey_2).unwrap();
    while 0 != address_2.get_thread(thread_count) {
        priv_2 = crypto::generate_random_private_key();
        pubkey_2 = crypto::derive_public_key(&priv_2);
        address_2 = Address::from_public_key(&pubkey_2).unwrap();
    }
    assert_eq!(0, address_2.get_thread(thread_count));

    let mut ledger = HashMap::new();
    ledger.insert(address_2, LedgerData { balance: 10_000 });
    let ledger_file = generate_ledger_file(&ledger);

    let staking_file = tools::generate_staking_keys_file(&vec![priv_1]);
    let roll_counts_file = tools::generate_default_roll_counts_file(vec![priv_1]);
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.periods_per_cycle = 2;
    cfg.pos_lookback_cycles = 2;
    cfg.pos_lock_cycles = 1;
    cfg.t0 = 500.into();
    cfg.delta_f0 = 3;
    cfg.disable_block_creation = false;
    cfg.thread_count = thread_count;
    cfg.operation_validity_periods = 10;
    cfg.operation_batch_size = 500;
    cfg.max_operations_per_block = 5000;
    cfg.max_block_size = 500;
    cfg.block_reward = 0;
    cfg.roll_price = 1000;
    cfg.operation_validity_periods = 100;

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let (mut pool_controller, pool_command_sender) = MockPoolController::new();

    cfg.genesis_timestamp = UTime::now(0).unwrap().saturating_add(300.into());
    // launch consensus controller
    let (consensus_command_sender, _consensus_event_receiver, _consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
            pool_command_sender,
            None,
            None,
            None,
            0,
        )
        .await
        .expect("could not start consensus controller");

    // operations
    let rb_a2_r1 = create_roll_buy(priv_2, 1, 90, 0);
    let rs_a2_r1 = create_roll_sell(priv_2, 1, 90, 0);

    let mut addresses = HashSet::new();
    addresses.insert(address_2);
    let addresses = addresses;

    //wait for first slot
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
            _ => None,
        })
        .await
        .expect("timeout while waiting for 1st operation batch request");

    // wait for block
    let (_block_id, block) = protocol_controller
        .wait_command(500.into(), |cmd| match cmd {
            ProtocolCommand::IntegratedBlock { block_id, block } => Some((block_id, block)),
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
    assert_eq!(addr_state.active_rolls, Some(0));
    assert_eq!(addr_state.final_rolls, 0);
    assert_eq!(addr_state.candidate_rolls, 1);

    let balance = consensus_command_sender
        .get_addresses_info(addresses.clone())
        .await
        .unwrap()
        .get(&address_2)
        .unwrap()
        .candidate_ledger_data
        .balance;
    assert_eq!(balance, 9000);

    wait_pool_slot(&mut &mut pool_controller, cfg.t0, 1, 1).await;
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
            _ => None,
        })
        .await
        .expect("timeout while waiting for operation batch request");

    // wait for block
    let (_block_id, block) = protocol_controller
        .wait_command(500.into(), |cmd| match cmd {
            ProtocolCommand::IntegratedBlock { block_id, block } => Some((block_id, block)),
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
            _ => None,
        })
        .await
        .expect("timeout while waiting for 1st operation batch request");

    // wait for block
    let (_block_id, block) = protocol_controller
        .wait_command(500.into(), |cmd| match cmd {
            ProtocolCommand::IntegratedBlock { block_id, block } => Some((block_id, block)),
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
    assert_eq!(addr_state.active_rolls, Some(0));
    assert_eq!(addr_state.final_rolls, 0);
    assert_eq!(addr_state.candidate_rolls, 0);
    let balance = addr_state.candidate_ledger_data.balance;
    assert_eq!(balance, 9000);
}
