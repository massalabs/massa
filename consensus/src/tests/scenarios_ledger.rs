use super::tools;
use crate::ledger::{Ledger, LedgerChange};
use models::Address;

#[tokio::test]
async fn test_ledger_init() {
    let (cfg, serialization_context) = tools::default_consensus_config(1);
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone());
    assert!(ledger.is_ok());
}

#[tokio::test]
async fn test_ledger_initializes_get_latest_final_periods() {
    let (cfg, serialization_context) = tools::default_consensus_config(1);
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone()).unwrap();

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
    let (cfg, serialization_context) = tools::default_consensus_config(1);
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone()).unwrap();

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
    let (cfg, serialization_context) = tools::default_consensus_config(1);
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone()).unwrap();

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
    let (cfg, serialization_context) = tools::default_consensus_config(1);
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone()).unwrap();

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
    let (cfg, serialization_context) = tools::default_consensus_config(1);
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone()).unwrap();

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
    let (cfg, serialization_context) = tools::default_consensus_config(1);
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone()).unwrap();

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
    let (cfg, serialization_context) = tools::default_consensus_config(1);
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone()).unwrap();

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
    let (cfg, serialization_context) = tools::default_consensus_config(1);
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone()).unwrap();

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
    let (cfg, serialization_context) = tools::default_consensus_config(1);
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone()).unwrap();

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
    let (cfg, serialization_context) = tools::default_consensus_config(1);
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone()).unwrap();

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
    let (cfg, serialization_context) = tools::default_consensus_config(1);
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone()).unwrap();

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
    let (cfg, serialization_context) = tools::default_consensus_config(1);
    let ledger = Ledger::new(cfg.clone(), serialization_context.clone()).unwrap();

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
        .get(&address)
        .expect("Couldn't find ledger data for address.");
    assert_eq!(address_data.get_balance(), 1);
}
