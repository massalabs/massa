use std::collections::HashMap;

use super::tools;
use crate::{
    ledger::{Ledger, LedgerChange},
    tests::tools::generate_ledger_file,
};
use models::Address;

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
