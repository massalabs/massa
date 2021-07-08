use std::collections::HashMap;

use super::{
    mock_pool_controller::{MockPoolController, PoolCommandSink},
    mock_protocol_controller::MockProtocolController,
    tools,
};
use crate::{
    ledger::{Ledger, LedgerChange, LedgerData},
    random_selector::RandomSelector,
    start_consensus_controller,
    tests::tools::{create_block_with_operations, create_transaction, generate_ledger_file},
};
use models::{Address, Slot};
use time::UTime;

#[tokio::test]
async fn test_ledger_init() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let (cfg, serialization_context) = tools::default_consensus_config(1, ledger_file.path());
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone(), None);
    assert!(ledger.is_ok());
}

#[tokio::test]
async fn test_ledger_initializes_get_latest_final_periods() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let (cfg, serialization_context) = tools::default_consensus_config(1, ledger_file.path());
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone(), None).unwrap();

    let private_key = crypto::generate_random_private_key();
    let public_key = crypto::derive_public_key(&private_key);
    let address = Address::from_public_key(&public_key).unwrap();

    for latest_final in ledger
        .get_latest_final_periods()
        .expect("Couldn't get final periods.")
    {
        assert_eq!(latest_final, 0);
    }
}

#[tokio::test]
async fn test_ledger_final_balance_increment_new_address() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let (cfg, serialization_context) = tools::default_consensus_config(1, ledger_file.path());
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone(), None).unwrap();

    let private_key = crypto::generate_random_private_key();
    let public_key = crypto::derive_public_key(&private_key);
    let address = Address::from_public_key(&public_key).unwrap();
    let thread = address.get_thread(cfg.thread_count);

    let change = LedgerChange::new(1, true);
    ledger
        .apply_final_changes(thread, vec![(address.clone(), change)], 1)
        .expect("Couldn't apply final changes");

    let final_datas = ledger
        .get_final_datas(vec![&address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(final_data_for_address.get_balance(), 1);
}

#[tokio::test]
async fn test_ledger_apply_change_wrong_thread() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let (cfg, serialization_context) = tools::default_consensus_config(1, ledger_file.path());
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone(), None).unwrap();

    let private_key = crypto::generate_random_private_key();
    let public_key = crypto::derive_public_key(&private_key);
    let address = Address::from_public_key(&public_key).unwrap();
    let thread = address.get_thread(cfg.thread_count);

    let change = LedgerChange::new(1, true);

    // Note: wrong thread.
    assert!(ledger
        .apply_final_changes(thread + 1, vec![(address.clone(), change)], 1)
        .is_err());

    // Balance should still be zero.
    let final_datas = ledger
        .get_final_datas(vec![&address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(final_data_for_address.get_balance(), 0);
}

#[tokio::test]
async fn test_ledger_final_balance_increment_address_above_max() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let (cfg, serialization_context) = tools::default_consensus_config(1, ledger_file.path());
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone(), None).unwrap();

    let private_key = crypto::generate_random_private_key();
    let public_key = crypto::derive_public_key(&private_key);
    let address = Address::from_public_key(&public_key).unwrap();
    let thread = address.get_thread(cfg.thread_count);

    let change = LedgerChange::new(1, true);
    ledger
        .apply_final_changes(thread, vec![(address.clone(), change)], 1)
        .expect("Couldn't apply final changes");

    let final_datas = ledger
        .get_final_datas(vec![&address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(final_data_for_address.get_balance(), 1);

    let change = LedgerChange::new(u64::MAX, true);
    assert!(ledger
        .apply_final_changes(thread, vec![(address.clone(), change)], 1)
        .is_err());
}

#[tokio::test]
async fn test_ledger_final_balance_decrement_address_balance_to_zero() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let (cfg, serialization_context) = tools::default_consensus_config(1, ledger_file.path());
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone(), None).unwrap();

    let private_key = crypto::generate_random_private_key();
    let public_key = crypto::derive_public_key(&private_key);
    let address = Address::from_public_key(&public_key).unwrap();
    let thread = address.get_thread(cfg.thread_count);

    // Increment.
    let change = LedgerChange::new(1, true);
    ledger
        .apply_final_changes(thread, vec![(address.clone(), change)], 1)
        .expect("Couldn't apply final changes");

    let final_datas = ledger
        .get_final_datas(vec![&address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(final_data_for_address.get_balance(), 1);

    // Decrement.
    let change = LedgerChange::new(1, false);
    ledger
        .apply_final_changes(thread, vec![(address.clone(), change)], 1)
        .expect("Couldn't apply final changes");

    let final_datas = ledger
        .get_final_datas(vec![&address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(final_data_for_address.get_balance(), 0);
}

#[tokio::test]
async fn test_ledger_final_balance_decrement_address_below_zero() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let (cfg, serialization_context) = tools::default_consensus_config(1, ledger_file.path());
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone(), None).unwrap();

    let private_key = crypto::generate_random_private_key();
    let public_key = crypto::derive_public_key(&private_key);
    let address = Address::from_public_key(&public_key).unwrap();
    let thread = address.get_thread(cfg.thread_count);

    // Increment.
    let change = LedgerChange::new(1, true);
    ledger
        .apply_final_changes(thread, vec![(address.clone(), change)], 1)
        .expect("Couldn't apply final changes");

    let final_datas = ledger
        .get_final_datas(vec![&address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(final_data_for_address.get_balance(), 1);

    // Decrement.
    let change = LedgerChange::new(1, false);
    ledger
        .apply_final_changes(thread, vec![(address.clone(), change)], 1)
        .expect("Couldn't apply final changes");

    let final_datas = ledger
        .get_final_datas(vec![&address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(final_data_for_address.get_balance(), 0);

    // Try to decrement again.
    let change = LedgerChange::new(1, false);
    assert!(ledger
        .apply_final_changes(thread, vec![(address.clone(), change)], 1)
        .is_err());
}

#[tokio::test]
async fn test_ledger_final_balance_decrement_non_existing_address() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let (cfg, serialization_context) = tools::default_consensus_config(1, ledger_file.path());
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone(), None).unwrap();

    let private_key = crypto::generate_random_private_key();
    let public_key = crypto::derive_public_key(&private_key);
    let address = Address::from_public_key(&public_key).unwrap();
    let thread = address.get_thread(cfg.thread_count);

    // Decrement.
    let change = LedgerChange::new(1, false);
    assert!(ledger
        .apply_final_changes(thread, vec![(address.clone(), change)], 1)
        .is_err());
}

#[tokio::test]
async fn test_ledger_final_balance_non_existing_address() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let (cfg, serialization_context) = tools::default_consensus_config(1, ledger_file.path());
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone(), None).unwrap();

    let private_key = crypto::generate_random_private_key();
    let public_key = crypto::derive_public_key(&private_key);
    let address = Address::from_public_key(&public_key).unwrap();

    let final_datas = ledger
        .get_final_datas(vec![&address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(final_data_for_address.get_balance(), 0);
}

#[tokio::test]
async fn test_ledger_final_balance_duplicate_address() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let (cfg, serialization_context) = tools::default_consensus_config(1, ledger_file.path());
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone(), None).unwrap();

    let private_key = crypto::generate_random_private_key();
    let public_key = crypto::derive_public_key(&private_key);
    let address = Address::from_public_key(&public_key).unwrap();

    // Same address twice.
    let final_datas = ledger
        .get_final_datas(vec![&address, &address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(final_data_for_address.get_balance(), 0);

    // Should have returned a single result.
    assert_eq!(final_datas.len(), 1);
}

#[tokio::test]
async fn test_ledger_final_balance_multiple_addresses() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let (cfg, serialization_context) = tools::default_consensus_config(1, ledger_file.path());
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone(), None).unwrap();

    let mut addresses = vec![];
    for _ in 0..5 {
        let private_key = crypto::generate_random_private_key();
        let public_key = crypto::derive_public_key(&private_key);
        let address = Address::from_public_key(&public_key).unwrap();
        addresses.push(address);
    }

    let final_datas = ledger
        .get_final_datas(addresses.iter().collect())
        .expect("Couldn't get final balance.");

    assert_eq!(final_datas.len(), addresses.len());

    for address in addresses {
        let final_data_for_address = final_datas
            .get(&address)
            .expect("Couldn't get data for address.");
        assert_eq!(final_data_for_address.get_balance(), 0);
    }
}

#[tokio::test]
async fn test_ledger_clear() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let (cfg, serialization_context) = tools::default_consensus_config(1, ledger_file.path());
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone(), None).unwrap();

    let private_key = crypto::generate_random_private_key();
    let public_key = crypto::derive_public_key(&private_key);
    let address = Address::from_public_key(&public_key).unwrap();
    let thread = address.get_thread(cfg.thread_count);

    let change = LedgerChange::new(1, true);
    ledger
        .apply_final_changes(thread, vec![(address.clone(), change)], 1)
        .expect("Couldn't apply final changes");

    let final_datas = ledger
        .get_final_datas(vec![&address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(final_data_for_address.get_balance(), 1);

    ledger.clear().expect("Couldn't clear the ledger.");

    let final_datas = ledger
        .get_final_datas(vec![&address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(final_data_for_address.get_balance(), 0);
}

#[tokio::test]
async fn test_ledger_read_whole() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let (cfg, serialization_context) = tools::default_consensus_config(1, ledger_file.path());
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone(), None).unwrap();

    let private_key = crypto::generate_random_private_key();
    let public_key = crypto::derive_public_key(&private_key);
    let address = Address::from_public_key(&public_key).unwrap();
    let thread = address.get_thread(cfg.thread_count);

    let change = LedgerChange::new(1, true);
    ledger
        .apply_final_changes(thread, vec![(address.clone(), change)], 1)
        .expect("Couldn't apply final changes");

    let final_datas = ledger
        .get_final_datas(vec![&address].into_iter().collect())
        .expect("Couldn't get final balance.");
    let final_data_for_address = final_datas
        .get(&address)
        .expect("Couldn't get data for address.");
    assert_eq!(final_data_for_address.get_balance(), 1);

    let whole = ledger.read_whole().expect("Couldn't read whole ledger.");
    let thread_ledger = whole
        .get(thread as usize)
        .expect("Couldn't get ledger for thread.");
    let address_data = thread_ledger
        .iter()
        .filter(|(addr, _)| addr.clone() == address)
        .collect::<Vec<_>>()
        .pop()
        .expect("Couldn't find ledger data for address.")
        .1
        .clone();
    assert_eq!(address_data.get_balance(), 1);
}

#[tokio::test]
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
        private_key_1 = crypto::generate_random_private_key();
        public_key_1 = crypto::derive_public_key(&private_key_1);
        address_1 = Address::from_public_key(&public_key_1).unwrap();
        if address_1.get_thread(thread_count) == 0 {
            break;
        }
    }
    loop {
        // B
        private_key_2 = crypto::generate_random_private_key();
        public_key_2 = crypto::derive_public_key(&private_key_2);
        address_2 = Address::from_public_key(&public_key_2).unwrap();
        if address_2.get_thread(thread_count) == 1 {
            break;
        }
    }
    loop {
        // C
        private_key_3 = crypto::generate_random_private_key();
        public_key_3 = crypto::derive_public_key(&private_key_3);
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
    ledger.insert(address_1, LedgerData { balance: 1000 });
    ledger.insert(address_2, LedgerData { balance: 3000 });

    let ledger_file = generate_ledger_file(&ledger);
    let (mut cfg, serialization_context) = tools::default_consensus_config(1, ledger_file.path());
    cfg.t0 = 1000.into();
    cfg.genesis_timestamp = UTime::now(0)
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(10).unwrap());
    cfg.delta_f0 = 4;
    cfg.block_reward = 1;
    cfg.operation_validity_periods = 20;
    let nodes = vec![(public_key_1, private_key_1)];
    let addresses = vec![address_1, address_2, address_3];
    cfg.nodes = nodes.clone();

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new(serialization_context.clone());
    let (pool_controller, pool_command_sender) =
        MockPoolController::new(serialization_context.clone());
    let _pool_sink = PoolCommandSink::new(pool_controller).await;

    // launch consensus controller
    let (consensus_command_sender, _consensus_event_receiver, _consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            serialization_context.clone(),
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
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    // A -> B [amount 10, fee 3]
    let operation_1 = create_transaction(
        private_key_1,
        public_key_1,
        address_2,
        10,
        &serialization_context,
        10,
        3,
    );

    // Add block B3
    let (block_a_id, block_a, _) = create_block_with_operations(
        &cfg,
        &serialization_context,
        Slot::new(1, 0),
        &genesis_ids,
        nodes[0],
        vec![operation_1],
    );
    protocol_controller.receive_block(block_a).await;
    tools::validate_propagate_block(&mut protocol_controller, block_a_id, 150).await;

    // B -> A [amount 9, fee 2]
    let operation_2 = create_transaction(
        private_key_2,
        public_key_2,
        address_1,
        9,
        &serialization_context,
        10,
        2,
    );

    // B -> C [amount 3, fee 1]
    let operation_3 = create_transaction(
        private_key_2,
        public_key_2,
        address_3,
        3,
        &serialization_context,
        10,
        1,
    );

    // Add block B4
    let (block_b_id, block_b, _) = create_block_with_operations(
        &cfg,
        &serialization_context,
        Slot::new(1, 1),
        &genesis_ids,
        nodes[0],
        vec![operation_2, operation_3],
    );
    protocol_controller.receive_block(block_b).await;
    tools::validate_propagate_block(&mut protocol_controller, block_b_id, 150).await;

    // A -> C [amount 3, fee 4]
    let operation_4 = create_transaction(
        private_key_1,
        public_key_1,
        address_3,
        3,
        &serialization_context,
        10,
        4,
    );

    // Add block B5
    let (block_c_id, block_c, _) = create_block_with_operations(
        &cfg,
        &serialization_context,
        Slot::new(2, 0),
        &vec![block_a_id, block_b_id],
        nodes[0],
        vec![operation_4],
    );
    protocol_controller.receive_block(block_c).await;
    tools::validate_propagate_block(&mut protocol_controller, block_c_id, 150).await;

    // Add block B6, no operations.
    let (block_d_id, block_d, _) = create_block_with_operations(
        &cfg,
        &serialization_context,
        Slot::new(2, 1),
        &vec![block_a_id, block_b_id],
        nodes[0],
        vec![],
    );
    protocol_controller.receive_block(block_d).await;
    tools::validate_propagate_block(&mut protocol_controller, block_d_id, 150).await;

    // A -> B [amount 11, fee 7]
    let operation_5 = create_transaction(
        private_key_1,
        public_key_1,
        address_2,
        11,
        &serialization_context,
        10,
        7,
    );
    // Add block B7
    let (block_e_id, block_e, _) = create_block_with_operations(
        &cfg,
        &serialization_context,
        Slot::new(3, 0),
        &vec![block_c_id, block_b_id],
        nodes[0],
        vec![operation_5],
    );
    protocol_controller.receive_block(block_e).await;
    tools::validate_propagate_block(&mut protocol_controller, block_e_id, 150).await;

    // B -> A [amount 17, fee 4]
    let operation_6 = create_transaction(
        private_key_2,
        public_key_2,
        address_1,
        17,
        &serialization_context,
        10,
        4,
    );
    // Add block B8
    let (block_f_id, block_f, _) = create_block_with_operations(
        &cfg,
        &serialization_context,
        Slot::new(3, 1),
        &vec![block_c_id, block_d_id],
        nodes[0],
        vec![operation_6],
    );
    protocol_controller.receive_block(block_f).await;
    tools::validate_propagate_block(&mut protocol_controller, block_f_id, 150).await;

    // Add block B9
    let (block_g_id, block_g, _) = create_block_with_operations(
        &cfg,
        &serialization_context,
        Slot::new(4, 0),
        &vec![block_e_id, block_f_id],
        nodes[0],
        vec![],
    );
    protocol_controller.receive_block(block_g).await;
    tools::validate_propagate_block(&mut protocol_controller, block_g_id, 150).await;

    // B3 and B4 have become final.
    {
        let ledger = consensus_command_sender
            .get_bootstrap_graph()
            .await
            .unwrap()
            .ledger;
        let ledger_0: HashMap<Address, LedgerData> =
            ledger.ledger_per_thread[0].iter().cloned().collect();
        let ledger_1: HashMap<Address, LedgerData> =
            ledger.ledger_per_thread[1].iter().cloned().collect();
        assert_eq!(
            ledger_0[&address_1].get_balance(),
            991,
            "wrong address balance"
        );
        assert_eq!(
            ledger_1[&address_2].get_balance(),
            2985,
            "wrong address balance"
        );
        assert!(
            !ledger_0.contains_key(&address_3),
            "address shouldn't be present"
        );
    }

    // Add block B10
    let (block_h_id, block_h, _) = create_block_with_operations(
        &cfg,
        &serialization_context,
        Slot::new(5, 0),
        &vec![block_g_id, block_f_id],
        nodes[0],
        vec![],
    );
    protocol_controller.receive_block(block_h).await;
    tools::validate_propagate_block(&mut protocol_controller, block_h_id, 150).await;

    // Add block B11
    let (block_i_id, block_i, _) = create_block_with_operations(
        &cfg,
        &serialization_context,
        Slot::new(6, 0),
        &vec![block_h_id, block_f_id],
        nodes[0],
        vec![],
    );
    protocol_controller.receive_block(block_i).await;
    tools::validate_propagate_block(&mut protocol_controller, block_i_id, 150).await;

    // B5 has become final.
    {
        let ledger = consensus_command_sender
            .get_bootstrap_graph()
            .await
            .unwrap()
            .ledger;
        let ledger_0: HashMap<Address, LedgerData> =
            ledger.ledger_per_thread[0].iter().cloned().collect();
        let ledger_1: HashMap<Address, LedgerData> =
            ledger.ledger_per_thread[1].iter().cloned().collect();
        assert_eq!(
            ledger_0[&address_1].get_balance(),
            1002,
            "wrong address balance"
        );
        assert_eq!(
            ledger_1[&address_2].get_balance(),
            2985,
            "wrong address balance"
        );
        assert_eq!(
            ledger_0[&address_3].get_balance(),
            6,
            "wrong address balance"
        );
    }

    // Add block B12
    let (block_j_id, block_j, _) = create_block_with_operations(
        &cfg,
        &serialization_context,
        Slot::new(7, 0),
        &vec![block_i_id, block_f_id],
        nodes[0],
        vec![],
    );
    protocol_controller.receive_block(block_j).await;
    tools::validate_propagate_block(&mut protocol_controller, block_j_id, 150).await;

    // B6 has become final.
    {
        let ledger = consensus_command_sender
            .get_bootstrap_graph()
            .await
            .unwrap()
            .ledger;
        let ledger_0: HashMap<Address, LedgerData> =
            ledger.ledger_per_thread[0].iter().cloned().collect();
        let ledger_1: HashMap<Address, LedgerData> =
            ledger.ledger_per_thread[1].iter().cloned().collect();
        assert_eq!(
            ledger_0[&address_1].get_balance(),
            1002,
            "wrong address balance"
        );
        assert_eq!(
            ledger_1[&address_2].get_balance(),
            2995,
            "wrong address balance"
        );
        assert_eq!(
            ledger_0[&address_3].get_balance(),
            6,
            "wrong address balance"
        );
    }

    // Add block B13
    let (block_k_id, block_k, _) = create_block_with_operations(
        &cfg,
        &serialization_context,
        Slot::new(8, 0),
        &vec![block_j_id, block_f_id],
        nodes[0],
        vec![],
    );
    protocol_controller.receive_block(block_k).await;
    tools::validate_propagate_block(&mut protocol_controller, block_k_id, 150).await;

    // B7 and B8 have become final.
    {
        let ledger = consensus_command_sender
            .get_bootstrap_graph()
            .await
            .unwrap()
            .ledger;
        let ledger_0: HashMap<Address, LedgerData> =
            ledger.ledger_per_thread[0].iter().cloned().collect();
        let ledger_1: HashMap<Address, LedgerData> =
            ledger.ledger_per_thread[1].iter().cloned().collect();
        assert_eq!(
            ledger_0[&address_1].get_balance(),
            992,
            "wrong address balance"
        );
        assert_eq!(
            ledger_1[&address_2].get_balance(),
            2974,
            "wrong address balance"
        );
        assert_eq!(
            ledger_0[&address_3].get_balance(),
            6,
            "wrong address balance"
        );
    }
}
