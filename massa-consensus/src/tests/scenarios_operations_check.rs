// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::tests::tools::{
    self, create_block_with_operations, create_transaction, generate_ledger_file, propagate_block,
};
use massa_models::ledger::LedgerData;
use massa_models::prehash::Set;
use massa_models::{Address, Amount, Slot};
use massa_signature::{derive_public_key, generate_random_private_key, PrivateKey};
use serial_test::serial;
use std::collections::HashMap;
use std::str::FromStr;

#[tokio::test]
#[serial]
async fn test_operations_check() {
    // setup logging
    /*
    stderrlog::new()
        .verbosity(4)
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()
        .unwrap();
    */

    let thread_count = 2;

    let mut private_key_1;
    let mut public_key_1;
    let mut address_1;
    let mut private_key_2;
    let mut public_key_2;
    let mut address_2;

    // make sure that both threads are different
    loop {
        private_key_1 = generate_random_private_key();
        public_key_1 = derive_public_key(&private_key_1);
        address_1 = Address::from_public_key(&public_key_1);
        if address_1.get_thread(thread_count) == 0 {
            break;
        }
    }
    loop {
        private_key_2 = generate_random_private_key();
        public_key_2 = derive_public_key(&private_key_2);
        address_2 = Address::from_public_key(&public_key_2);
        if address_2.get_thread(thread_count) == 1 {
            break;
        }
    }

    let mut ledger = HashMap::new();
    ledger.insert(address_1, LedgerData::new(Amount::from_str("5").unwrap()));

    let ledger_file = generate_ledger_file(&ledger);
    let staking_keys: Vec<PrivateKey> = vec![private_key_1];
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.t0 = 1000.into();
    cfg.future_block_processing_max_periods = 50;
    cfg.max_future_processing_blocks = 10;
    cfg.block_reward = Amount::from_str("1").unwrap();
    cfg.thread_count = thread_count;
    cfg.operation_validity_periods = 10;
    cfg.disable_block_creation = true;
    cfg.genesis_timestamp = cfg.genesis_timestamp.saturating_sub(10000.into());

    tools::consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let genesis_ids = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .genesis_blocks;

            // Valid block A sending 5 from addr1 to addr2 + reward 1 to addr1
            let operation_1 = create_transaction(private_key_1, public_key_1, address_2, 5, 5, 1);
            let (id_a, block_a, _) = create_block_with_operations(
                &cfg,
                Slot::new(1, 0),
                &genesis_ids,
                private_key_1,
                vec![operation_1.clone()],
            );
            propagate_block(&mut protocol_controller, block_a, true, 150).await;

            // assert address 1 has 1 coin at blocks (A, genesis_ids[1]) (see #269)
            let mut set = Set::<Address>::default();
            set.insert(address_1);
            let res = consensus_command_sender
                .get_addresses_info(set)
                .await
                .unwrap()
                .get(&address_1)
                .unwrap()
                .ledger_info
                .candidate_ledger_info;
            assert_eq!(res.balance, Amount::from_str("1").unwrap());

            // receive block b with invalid operation (not enough coins)
            let operation_2 = create_transaction(private_key_2, public_key_2, address_1, 10, 8, 1);
            let (_, block_2b, _) = create_block_with_operations(
                &cfg,
                Slot::new(1, 1),
                &vec![id_a, genesis_ids[1]],
                private_key_1,
                vec![operation_2],
            );
            propagate_block(&mut protocol_controller, block_2b, false, 1000).await;

            // receive empty block b
            let (id_b, block_b, _) = create_block_with_operations(
                &cfg,
                Slot::new(1, 1),
                &vec![id_a, genesis_ids[1]],
                private_key_1,
                vec![],
            );
            propagate_block(&mut protocol_controller, block_b, true, 150).await;

            // assert address 2 has 5 coins at block B
            let mut set = Set::<Address>::default();
            set.insert(address_2);
            let res = consensus_command_sender
                .get_addresses_info(set)
                .await
                .unwrap()
                .get(&address_2)
                .unwrap()
                .ledger_info
                .candidate_ledger_info;
            assert_eq!(res.balance, Amount::from_str("5").unwrap());

            // receive block with reused operation
            let (_, block_1c, _) = create_block_with_operations(
                &cfg,
                Slot::new(1, 0),
                &vec![id_a, id_b],
                private_key_1,
                vec![operation_1.clone()],
            );
            propagate_block(&mut protocol_controller, block_1c, false, 1000).await;

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
async fn test_execution_check() {
    let thread_count = 2;

    let private_key_1 = generate_random_private_key();
    let public_key_1 = derive_public_key(&private_key_1);
    let address_1 = Address::from_public_key(&public_key_1);

    let mut ledger = HashMap::new();
    ledger.insert(address_1, LedgerData::new(Amount::from_str("5").unwrap()));

    let ledger_file = generate_ledger_file(&ledger);
    let staking_keys: Vec<PrivateKey> = vec![private_key_1];
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.t0 = 1000.into();
    cfg.future_block_processing_max_periods = 50;
    cfg.max_future_processing_blocks = 10;
    cfg.block_reward = Amount::from_str("1").unwrap();
    cfg.thread_count = thread_count;
    cfg.operation_validity_periods = 10;
    cfg.disable_block_creation = true;
    cfg.genesis_timestamp = cfg.genesis_timestamp.saturating_sub(10000.into());

    tools::consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let genesis_ids = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .genesis_blocks;

            // Valid block A executing some bytecode and spending 2 coins.
            let operation_1 = tools::create_executesc(
                private_key_1,
                public_key_1,
                5,
                5,
                Default::default(),
                1,
                2,
                1,
            );
            let (_id_a, block_a, _) = create_block_with_operations(
                &cfg,
                Slot::new(1, 0),
                &genesis_ids,
                private_key_1,
                vec![operation_1.clone()],
            );
            propagate_block(&mut protocol_controller, block_a, true, 150).await;

            // assert the `coins` argument as been deducted from the balance of address 1.
            let mut set = Set::<Address>::default();
            set.insert(address_1);
            let res = consensus_command_sender
                .get_addresses_info(set)
                .await
                .unwrap()
                .get(&address_1)
                .unwrap()
                .ledger_info
                .candidate_ledger_info;
            assert_eq!(res.balance, Amount::from_str("3").unwrap());

            (
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}
