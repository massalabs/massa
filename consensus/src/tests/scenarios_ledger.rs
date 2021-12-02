// Copyright (c) 2021 MASSA LABS <info@massa.net>

use super::{
    mock_pool_controller::{MockPoolController, PoolCommandSink},
    mock_protocol_controller::MockProtocolController,
    tools,
};
use crate::{
    ledger::{Ledger, LedgerChanges},
    start_consensus_controller,
    tests::tools::{create_block_with_operations, create_transaction, generate_ledger_file},
};
use models::ledger::LedgerChange;
use models::ledger::LedgerData;
use models::{Address, Amount, Slot};
use serial_test::serial;
use signature::{derive_public_key, generate_random_private_key, PrivateKey};
use std::collections::HashMap;
use std::str::FromStr;
use time::UTime;

#[tokio::test]
#[serial]
async fn test_ledger_init() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys);
    let cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    let ledger = Ledger::new(cfg, None);
    assert!(ledger.is_ok());
}

#[tokio::test]
#[serial]
async fn test_ledger_initializes_get_latest_final_periods() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys);
    let cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    let ledger = Ledger::new(cfg, None).unwrap();

    for latest_final in ledger
        .get_latest_final_periods()
        .expect("Couldn't get final periods.")
    {
        assert_eq!(latest_final, 0);
    }
}

#[tokio::test]
#[serial]
async fn test_ledger_final_balance_increment_new_address() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys);
    let cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    let ledger = Ledger::new(cfg.clone(), None).unwrap();

    let private_key = generate_random_private_key();
    let public_key = derive_public_key(&private_key);
    let address = Address::from_public_key(&public_key).unwrap();
    let thread = address.get_thread(cfg.thread_count);

    let changes = LedgerChanges(
        vec![(
            address,
            LedgerChange {
                balance_delta: Amount::from_str("1").unwrap(),
                balance_increment: true,
            },
        )]
        .into_iter()
        .collect(),
    );
    ledger
        .apply_final_changes(thread, &changes, 1)
        .expect("Couldn't apply final changes");

    let final_datas = ledger
        .get_final_data(vec![address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .0
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(
        final_data_for_address.balance,
        Amount::from_str("1").unwrap()
    );
}

#[tokio::test]
#[serial]
async fn test_ledger_final_balance_increment_address_above_max() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys);
    let cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    let ledger = Ledger::new(cfg.clone(), None).unwrap();

    let private_key = generate_random_private_key();
    let public_key = derive_public_key(&private_key);
    let address = Address::from_public_key(&public_key).unwrap();
    let thread = address.get_thread(cfg.thread_count);

    let changes = LedgerChanges(
        vec![(
            address,
            LedgerChange {
                balance_delta: Amount::from_str("1").unwrap(),
                balance_increment: true,
            },
        )]
        .into_iter()
        .collect(),
    );
    ledger
        .apply_final_changes(thread, &changes, 1)
        .expect("Couldn't apply final changes");

    let final_datas = ledger
        .get_final_data(vec![address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .0
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(
        final_data_for_address.balance,
        Amount::from_str("1").unwrap()
    );

    let changes = LedgerChanges(
        vec![(
            address,
            LedgerChange {
                balance_delta: Amount::from_raw(u64::MAX),
                balance_increment: true,
            },
        )]
        .into_iter()
        .collect(),
    );
    assert!(ledger.apply_final_changes(thread, &changes, 1).is_err());
}

#[tokio::test]
#[serial]
async fn test_ledger_final_balance_decrement_address_balance_to_zero() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys);
    let cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    let ledger = Ledger::new(cfg.clone(), None).unwrap();

    let private_key = generate_random_private_key();
    let public_key = derive_public_key(&private_key);
    let address = Address::from_public_key(&public_key).unwrap();
    let thread = address.get_thread(cfg.thread_count);

    // Increment.
    let changes = LedgerChanges(
        vec![(
            address,
            LedgerChange {
                balance_delta: Amount::from_str("1").unwrap(),
                balance_increment: true,
            },
        )]
        .into_iter()
        .collect(),
    );
    ledger
        .apply_final_changes(thread, &changes, 1)
        .expect("Couldn't apply final changes");

    let final_datas = ledger
        .get_final_data(vec![address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .0
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(
        final_data_for_address.balance,
        Amount::from_str("1").unwrap()
    );

    // Decrement.
    let changes = LedgerChanges(
        vec![(
            address,
            LedgerChange {
                balance_delta: Amount::from_str("1").unwrap(),
                balance_increment: false,
            },
        )]
        .into_iter()
        .collect(),
    );
    ledger
        .apply_final_changes(thread, &changes, 1)
        .expect("Couldn't apply final changes");

    let final_datas = ledger
        .get_final_data(vec![address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .0
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(final_data_for_address.balance, Amount::default());
}

#[tokio::test]
#[serial]
async fn test_ledger_final_balance_decrement_address_below_zero() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys);
    let cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    let ledger = Ledger::new(cfg.clone(), None).unwrap();

    let private_key = generate_random_private_key();
    let public_key = derive_public_key(&private_key);
    let address = Address::from_public_key(&public_key).unwrap();
    let thread = address.get_thread(cfg.thread_count);

    // Increment.
    let changes = LedgerChanges(
        vec![(
            address,
            LedgerChange {
                balance_delta: Amount::from_str("1").unwrap(),
                balance_increment: true,
            },
        )]
        .into_iter()
        .collect(),
    );
    ledger
        .apply_final_changes(thread, &changes, 1)
        .expect("Couldn't apply final changes");

    let final_datas = ledger
        .get_final_data(vec![address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .0
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(
        final_data_for_address.balance,
        Amount::from_str("1").unwrap()
    );

    // Decrement.
    let changes = LedgerChanges(
        vec![(
            address,
            LedgerChange {
                balance_delta: Amount::from_str("1").unwrap(),
                balance_increment: false,
            },
        )]
        .into_iter()
        .collect(),
    );
    ledger
        .apply_final_changes(thread, &changes, 1)
        .expect("Couldn't apply final changes");

    let final_datas = ledger
        .get_final_data(vec![address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .0
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(final_data_for_address.balance, Amount::default());

    // Try to decrement again.
    let changes = LedgerChanges(
        vec![(
            address,
            LedgerChange {
                balance_delta: Amount::from_str("1").unwrap(),
                balance_increment: false,
            },
        )]
        .into_iter()
        .collect(),
    );
    assert!(ledger.apply_final_changes(thread, &changes, 1).is_err());
}

#[tokio::test]
#[serial]
async fn test_ledger_final_balance_decrement_non_existing_address() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys);
    let cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    let ledger = Ledger::new(cfg.clone(), None).unwrap();

    let private_key = generate_random_private_key();
    let public_key = derive_public_key(&private_key);
    let address = Address::from_public_key(&public_key).unwrap();
    let thread = address.get_thread(cfg.thread_count);

    // Decrement.
    let changes = LedgerChanges(
        vec![(
            address,
            LedgerChange {
                balance_delta: Amount::from_str("1").unwrap(),
                balance_increment: false,
            },
        )]
        .into_iter()
        .collect(),
    );
    assert!(ledger.apply_final_changes(thread, &changes, 1).is_err());
}

#[tokio::test]
#[serial]
async fn test_ledger_final_balance_non_existing_address() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys);
    let cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    let ledger = Ledger::new(cfg, None).unwrap();

    let private_key = generate_random_private_key();
    let public_key = derive_public_key(&private_key);
    let address = Address::from_public_key(&public_key).unwrap();

    let final_datas = ledger
        .get_final_data(vec![address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .0
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(final_data_for_address.balance, Amount::default());
}

#[tokio::test]
#[serial]
async fn test_ledger_final_balance_duplicate_address() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    let ledger = Ledger::new(cfg, None).unwrap();

    let private_key = generate_random_private_key();
    let public_key = derive_public_key(&private_key);
    let address = Address::from_public_key(&public_key).unwrap();

    // Same address twice.
    let final_datas = ledger
        .get_final_data(vec![address, address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .0
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(final_data_for_address.balance, Amount::default());

    // Should have returned a single result.
    assert_eq!(final_datas.0.len(), 1);
}

#[tokio::test]
#[serial]
async fn test_ledger_final_balance_multiple_addresses() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys);
    let cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    let ledger = Ledger::new(cfg, None).unwrap();

    let mut addresses = vec![];
    for _ in 0..5 {
        let private_key = generate_random_private_key();
        let public_key = derive_public_key(&private_key);
        let address = Address::from_public_key(&public_key).unwrap();
        addresses.push(address);
    }

    let final_datas = ledger
        .get_final_data(addresses.iter().copied().collect())
        .expect("Couldn't get final balance.");

    assert_eq!(final_datas.0.len(), addresses.len());

    for address in addresses {
        let final_data_for_address = final_datas
            .0
            .get(&address)
            .expect("Couldn't get data for address.");
        assert_eq!(final_data_for_address.balance, Amount::default());
    }
}

#[tokio::test]
#[serial]
async fn test_ledger_clear() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys);
    let cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    let ledger = Ledger::new(cfg.clone(), None).unwrap();

    let private_key = generate_random_private_key();
    let public_key = derive_public_key(&private_key);
    let address = Address::from_public_key(&public_key).unwrap();
    let thread = address.get_thread(cfg.thread_count);

    let changes = LedgerChanges(
        vec![(
            address,
            LedgerChange {
                balance_delta: Amount::from_str("1").unwrap(),
                balance_increment: true,
            },
        )]
        .into_iter()
        .collect(),
    );
    ledger
        .apply_final_changes(thread, &changes, 1)
        .expect("Couldn't apply final changes");

    let final_datas = ledger
        .get_final_data(vec![address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .0
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(
        final_data_for_address.balance,
        Amount::from_str("1").unwrap()
    );

    ledger.clear().expect("Couldn't clear the ledger.");

    let final_datas = ledger
        .get_final_data(vec![address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .0
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(final_data_for_address.balance, Amount::default());
}

#[tokio::test]
#[serial]
async fn test_ledger_read_whole() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys);
    let cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    let ledger = Ledger::new(cfg.clone(), None).unwrap();

    let private_key = generate_random_private_key();
    let public_key = derive_public_key(&private_key);
    let address = Address::from_public_key(&public_key).unwrap();
    let thread = address.get_thread(cfg.thread_count);

    let changes = LedgerChanges(
        vec![(
            address,
            LedgerChange {
                balance_delta: Amount::from_str("1").unwrap(),
                balance_increment: true,
            },
        )]
        .into_iter()
        .collect(),
    );
    ledger
        .apply_final_changes(thread, &changes, 1)
        .expect("Couldn't apply final changes");

    let final_datas = ledger
        .get_final_data(vec![address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .0
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(
        final_data_for_address.balance,
        Amount::from_str("1").unwrap()
    );

    let whole_ledger = ledger.read_whole().expect("Couldn't read whole ledger.");
    let address_data = whole_ledger
        .0
        .iter()
        .filter(|(addr, _)| **addr == address)
        .collect::<Vec<_>>()
        .pop()
        .expect("Couldn't find ledger data for address.")
        .1;
    assert_eq!(address_data.balance, Amount::from_str("1").unwrap());
}

#[tokio::test]
#[serial]
async fn test_ledger_update_when_a_batch_of_blocks_becomes_final() {
    let thread_count = 2;

    let mut private_key_1;
    let mut public_key_1;
    let mut address_1;

    let mut private_key_2;
    let mut public_key_2;
    let mut address_2;

    let mut private_key_3;
    let mut public_key_3;
    let mut address_3;

    loop {
        // A
        private_key_1 = generate_random_private_key();
        public_key_1 = derive_public_key(&private_key_1);
        address_1 = Address::from_public_key(&public_key_1).unwrap();
        if address_1.get_thread(thread_count) == 0 {
            break;
        }
    }
    loop {
        // B
        private_key_2 = generate_random_private_key();
        public_key_2 = derive_public_key(&private_key_2);
        address_2 = Address::from_public_key(&public_key_2).unwrap();
        if address_2.get_thread(thread_count) == 1 {
            break;
        }
    }
    loop {
        // C
        private_key_3 = generate_random_private_key();
        public_key_3 = derive_public_key(&private_key_3);
        address_3 = Address::from_public_key(&public_key_3).unwrap();
        if address_3.get_thread(thread_count) == 0 {
            break;
        }
    }

    // Ledger at genesis:
    //
    // Thread 0:
    // address A balance = 1000
    // address C absent from ledger
    //
    // Thread 1:
    // address B balance = 3000
    let mut ledger = HashMap::new();
    ledger.insert(
        address_1,
        LedgerData::new(Amount::from_str("1000").unwrap()),
    );
    ledger.insert(
        address_2,
        LedgerData::new(Amount::from_str("3000").unwrap()),
    );

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
    cfg.genesis_timestamp = UTime::now(0)
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(10).unwrap());
    cfg.delta_f0 = 4;
    cfg.block_reward = Amount::from_str("1").unwrap();
    cfg.operation_validity_periods = 20;

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let (pool_controller, pool_command_sender) = MockPoolController::new();
    let pool_sink = PoolCommandSink::new(pool_controller).await;

    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
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

    let genesis_ids = consensus_command_sender
        .get_block_graph_status(None, None)
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    // A -> B [amount 10, fee 3]
    let operation_1 = create_transaction(private_key_1, public_key_1, address_2, 10, 10, 3);

    // Add block B3
    let (block_a_id, block_a, _) = create_block_with_operations(
        &cfg,
        Slot::new(1, 0),
        &genesis_ids,
        staking_keys[0],
        vec![operation_1],
    );
    protocol_controller.receive_block(block_a).await;
    tools::validate_propagate_block(&mut protocol_controller, block_a_id, 150).await;

    // B -> A [amount 9, fee 2]
    let operation_2 = create_transaction(private_key_2, public_key_2, address_1, 9, 10, 2);

    // B -> C [amount 3, fee 1]
    let operation_3 = create_transaction(private_key_2, public_key_2, address_3, 3, 10, 1);

    // Add block B4
    let (block_b_id, block_b, _) = create_block_with_operations(
        &cfg,
        Slot::new(1, 1),
        &genesis_ids,
        staking_keys[0],
        vec![operation_2, operation_3],
    );
    protocol_controller.receive_block(block_b).await;
    tools::validate_propagate_block(&mut protocol_controller, block_b_id, 150).await;

    // A -> C [amount 3, fee 4]
    let operation_4 = create_transaction(private_key_1, public_key_1, address_3, 3, 10, 4);

    // Add block B5
    let (block_c_id, block_c, _) = create_block_with_operations(
        &cfg,
        Slot::new(2, 0),
        &vec![block_a_id, block_b_id],
        staking_keys[0],
        vec![operation_4],
    );
    protocol_controller.receive_block(block_c).await;
    tools::validate_propagate_block(&mut protocol_controller, block_c_id, 150).await;

    // Add block B6, no operations.
    let (block_d_id, block_d, _) = create_block_with_operations(
        &cfg,
        Slot::new(2, 1),
        &vec![block_a_id, block_b_id],
        staking_keys[0],
        vec![],
    );
    protocol_controller.receive_block(block_d).await;
    tools::validate_propagate_block(&mut protocol_controller, block_d_id, 150).await;

    // A -> B [amount 11, fee 7]
    let operation_5 = create_transaction(private_key_1, public_key_1, address_2, 11, 10, 7);
    // Add block B7
    let (block_e_id, block_e, _) = create_block_with_operations(
        &cfg,
        Slot::new(3, 0),
        &vec![block_c_id, block_b_id],
        staking_keys[0],
        vec![operation_5],
    );
    protocol_controller.receive_block(block_e).await;
    tools::validate_propagate_block(&mut protocol_controller, block_e_id, 150).await;

    // B -> A [amount 17, fee 4]
    let operation_6 = create_transaction(private_key_2, public_key_2, address_1, 17, 10, 4);
    // Add block B8
    let (block_f_id, block_f, _) = create_block_with_operations(
        &cfg,
        Slot::new(3, 1),
        &vec![block_c_id, block_d_id],
        staking_keys[0],
        vec![operation_6],
    );
    protocol_controller.receive_block(block_f).await;
    tools::validate_propagate_block(&mut protocol_controller, block_f_id, 150).await;

    // Add block B9
    let (block_g_id, block_g, _) = create_block_with_operations(
        &cfg,
        Slot::new(4, 0),
        &vec![block_e_id, block_f_id],
        staking_keys[0],
        vec![],
    );
    protocol_controller.receive_block(block_g).await;
    tools::validate_propagate_block(&mut protocol_controller, block_g_id, 150).await;

    // B3 and B4 have become final.
    {
        let ledger = consensus_command_sender
            .get_bootstrap_state()
            .await
            .unwrap()
            .1
            .ledger;
        assert_eq!(
            ledger.0[&address_1].balance,
            Amount::from_str("991").unwrap(),
            "wrong address balance"
        );
        assert_eq!(
            ledger.0[&address_2].balance,
            Amount::from_str("2985").unwrap(),
            "wrong address balance"
        );
        assert!(
            !ledger.0.contains_key(&address_3),
            "address shouldn't be present"
        );
    }

    // Add block B10
    let (block_h_id, block_h, _) = create_block_with_operations(
        &cfg,
        Slot::new(5, 0),
        &vec![block_g_id, block_f_id],
        staking_keys[0],
        vec![],
    );
    protocol_controller.receive_block(block_h).await;
    tools::validate_propagate_block(&mut protocol_controller, block_h_id, 150).await;

    // Add block B11
    let (block_i_id, block_i, _) = create_block_with_operations(
        &cfg,
        Slot::new(6, 0),
        &vec![block_h_id, block_f_id],
        staking_keys[0],
        vec![],
    );
    protocol_controller.receive_block(block_i).await;
    tools::validate_propagate_block(&mut protocol_controller, block_i_id, 150).await;

    // B5 has become final.
    {
        let ledger = consensus_command_sender
            .get_bootstrap_state()
            .await
            .unwrap()
            .1
            .ledger;
        assert_eq!(
            ledger.0[&address_1].balance,
            Amount::from_str("1002").unwrap(),
            "wrong address balance"
        );
        assert_eq!(
            ledger.0[&address_2].balance,
            Amount::from_str("2985").unwrap(),
            "wrong address balance"
        );
        assert_eq!(
            ledger.0[&address_3].balance,
            Amount::from_str("6").unwrap(),
            "wrong address balance"
        );
    }

    // Add block B12
    let (block_j_id, block_j, _) = create_block_with_operations(
        &cfg,
        Slot::new(7, 0),
        &vec![block_i_id, block_f_id],
        staking_keys[0],
        vec![],
    );
    protocol_controller.receive_block(block_j).await;
    tools::validate_propagate_block(&mut protocol_controller, block_j_id, 150).await;

    // B6 has become final.
    {
        let ledger = consensus_command_sender
            .get_bootstrap_state()
            .await
            .unwrap()
            .1
            .ledger;
        assert_eq!(
            ledger.0[&address_1].balance,
            Amount::from_str("1002").unwrap(),
            "wrong address balance"
        );
        assert_eq!(
            ledger.0[&address_2].balance,
            Amount::from_str("2995").unwrap(),
            "wrong address balance"
        );
        assert_eq!(
            ledger.0[&address_3].balance,
            Amount::from_str("6").unwrap(),
            "wrong address balance"
        );
    }

    // Add block B13
    let (block_k_id, block_k, _) = create_block_with_operations(
        &cfg,
        Slot::new(8, 0),
        &vec![block_j_id, block_f_id],
        staking_keys[0],
        vec![],
    );
    protocol_controller.receive_block(block_k).await;
    tools::validate_propagate_block(&mut protocol_controller, block_k_id, 150).await;

    // B7 and B8 have become final.
    {
        let ledger = consensus_command_sender
            .get_bootstrap_state()
            .await
            .unwrap()
            .1
            .ledger;
        assert_eq!(
            ledger.0[&address_1].balance,
            Amount::from_str("992").unwrap(),
            "wrong address balance"
        );
        assert_eq!(
            ledger.0[&address_2].balance,
            Amount::from_str("2974").unwrap(),
            "wrong address balance"
        );
        assert_eq!(
            ledger.0[&address_3].balance,
            Amount::from_str("6").unwrap(),
            "wrong address balance"
        );
    }

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
    pool_sink.stop().await;
}
