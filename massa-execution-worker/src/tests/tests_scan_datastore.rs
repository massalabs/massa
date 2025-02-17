use std::{
    collections::{BTreeMap, VecDeque},
    ops::Bound,
    sync::Arc,
};

use massa_execution_exports::ExecutionOutput;
use massa_final_state::StateChanges;
use massa_ledger_exports::{LedgerChanges, LedgerEntry, LedgerEntryUpdate};
use massa_models::{address::Address, prehash::PreHashMap, slot::Slot};
use massa_signature::KeyPair;
use parking_lot::RwLock;
use rand::{distributions::Alphanumeric, seq::SliceRandom, thread_rng, Rng};

use crate::{active_history::ActiveHistory, datastore_scan::scan_datastore};

use super::universe::ExecutionForeignControllers;

#[test]
fn test_scan_datastore() {
    let keypair = KeyPair::generate(0).unwrap();
    let addr = Address::from_public_key(&keypair.get_public_key());

    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();

    foreign_controllers
        .ledger_controller
        .set_expectations(|ledger_controller| {
            ledger_controller
                .expect_get_datastore_keys()
                .returning(move |_, _, _, _, _| None);
        });

    foreign_controllers
        .final_state
        .write()
        .expect_get_ledger()
        .return_const(Box::new(foreign_controllers.ledger_controller.clone()));

    let active_history = Arc::new(RwLock::new(ActiveHistory(VecDeque::new())));

    let (final_keys, candidate_keys) = scan_datastore(
        &addr,
        &[],
        Bound::Included(b"1".to_vec()),
        Bound::Unbounded,
        None,
        foreign_controllers.final_state.clone(),
        active_history,
        None,
    );
    // no data in the datastore
    assert!(final_keys.is_none());
    assert!(candidate_keys.is_none());

    let mut data = BTreeMap::new();
    data.insert(b"1".to_vec(), b"a".to_vec());
    data.insert(b"11".to_vec(), b"a".to_vec());
    data.insert(b"111".to_vec(), b"a".to_vec());
    data.insert(b"12".to_vec(), b"a".to_vec());
    data.insert(b"13".to_vec(), b"a".to_vec());

    data.insert(b"2".to_vec(), b"b".to_vec());
    data.insert(b"21".to_vec(), b"a".to_vec());

    data.insert(b"3".to_vec(), b"c".to_vec());
    data.insert(b"34".to_vec(), b"c".to_vec());

    let mut changes = PreHashMap::default();

    changes.insert(
        addr,
        massa_models::types::SetUpdateOrDelete::Set(LedgerEntry {
            datastore: data.clone(),
            ..Default::default()
        }),
    );

    let exec_output = ExecutionOutput {
        slot: Slot::new(1, 0),
        block_info: None,
        state_changes: StateChanges {
            ledger_changes: LedgerChanges(changes.clone()),
            async_pool_changes: Default::default(),
            deferred_call_changes: Default::default(),
            pos_changes: Default::default(),
            executed_ops_changes: Default::default(),
            executed_denunciations_changes: Default::default(),
            execution_trail_hash_change: Default::default(),
        },
        events: Default::default(),
        #[cfg(feature = "execution-trace")]
        slot_trace: Default::default(),
        #[cfg(feature = "dump-block")]
        storage: None,
        deferred_credits_execution: Default::default(),
        cancel_async_message_execution: Default::default(),
        auto_sell_execution: Default::default(),
        transfers_history: Default::default(),
    };

    let active_history = Arc::new(RwLock::new(ActiveHistory(VecDeque::from([
        exec_output.clone()
    ]))));
    // active history contains set data
    let (final_keys, candidate_keys) = scan_datastore(
        &addr,
        &[],
        Bound::Included(b"12".to_vec()),
        Bound::Unbounded,
        None,
        foreign_controllers.final_state.clone(),
        active_history.clone(),
        None,
    );

    assert!(&final_keys.is_none());

    let mut candidate_k = candidate_keys.unwrap();
    // should start at key "12" and skip key "1", "11", "111"
    assert_eq!(candidate_k.pop_first().unwrap(), b"12".to_vec());
    assert_eq!(candidate_k.pop_first().unwrap(), b"13".to_vec());
    assert_eq!(candidate_k.pop_first().unwrap(), b"2".to_vec());
    assert_eq!(candidate_k.pop_first().unwrap(), b"21".to_vec());
    assert_eq!(candidate_k.pop_first().unwrap(), b"3".to_vec());
    assert_eq!(candidate_k.pop_first().unwrap(), b"34".to_vec());
    assert_eq!(candidate_k.pop_first(), None);

    // create added changes
    // delete key "2"
    let mut changes = PreHashMap::default();
    let mut datastore_update = BTreeMap::new();
    datastore_update.insert(b"2".to_vec(), massa_models::types::SetOrDelete::Delete);

    changes.insert(
        addr,
        massa_models::types::SetUpdateOrDelete::Update(LedgerEntryUpdate {
            datastore: datastore_update,
            ..Default::default()
        }),
    );

    let (final_keys, candidate_keys) = scan_datastore(
        &addr,
        &[],
        Bound::Included(b"12".to_vec()),
        Bound::Unbounded,
        Some(4),
        foreign_controllers.final_state.clone(),
        active_history.clone(),
        Some(&LedgerChanges(changes.clone())),
    );

    assert!(final_keys.is_none());

    let mut candidate_k = candidate_keys.unwrap();
    // result should be limited to 4 keys
    // should start at key "12" and not contains key "2"
    assert_eq!(candidate_k.pop_first().unwrap(), b"12".to_vec());
    assert_eq!(candidate_k.pop_first().unwrap(), b"13".to_vec());
    assert_eq!(candidate_k.pop_first().unwrap(), b"21".to_vec());
    assert_eq!(candidate_k.pop_first().unwrap(), b"3".to_vec());
    assert_eq!(candidate_k.pop_first(), None);

    // add key "4" in new changes
    let mut exec2 = exec_output.clone();
    let mut new_changes = PreHashMap::default();
    let mut datastore_update = BTreeMap::new();
    datastore_update.insert(
        b"4".to_vec(),
        massa_models::types::SetOrDelete::Set(b"valueKey4".to_vec()),
    );

    new_changes.insert(
        addr,
        massa_models::types::SetUpdateOrDelete::Update(LedgerEntryUpdate {
            datastore: datastore_update,
            ..Default::default()
        }),
    );

    exec2.state_changes.ledger_changes = LedgerChanges(new_changes);

    let active_history = Arc::new(RwLock::new(ActiveHistory(VecDeque::from([
        exec_output.clone(),
        exec2,
    ]))));

    let (_final_keys, candidate_keys) = scan_datastore(
        &addr,
        &[],
        Bound::Included(b"12".to_vec()),
        Bound::Unbounded,
        None,
        foreign_controllers.final_state,
        active_history.clone(),
        Some(&LedgerChanges(changes.clone())),
    );
    let mut candidate_k = candidate_keys.unwrap();

    // should start at key "12" and not contains key "2"
    // should contains new key "4"
    assert_eq!(candidate_k.pop_first().unwrap(), b"12".to_vec());
    assert_eq!(candidate_k.pop_first().unwrap(), b"13".to_vec());
    assert_eq!(candidate_k.pop_first().unwrap(), b"21".to_vec());
    assert_eq!(candidate_k.pop_first().unwrap(), b"3".to_vec());
    assert_eq!(candidate_k.pop_first().unwrap(), b"34".to_vec());
    assert_eq!(candidate_k.pop_first().unwrap(), b"4".to_vec());
    assert_eq!(candidate_k.pop_first(), None);
}

#[test]
fn test_scan_datastore_with_random_data() {
    let mut rng = thread_rng();
    for _ in 0..10 {
        let keys_count = rng.gen_range(15..50);
        scan_datastore_with_random_data(keys_count);
    }
}

fn scan_datastore_with_random_data(nb_keys: usize) {
    let keypair = KeyPair::generate(0).unwrap();
    let addr = Address::from_public_key(&keypair.get_public_key());

    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();

    foreign_controllers
        .ledger_controller
        .set_expectations(|ledger_controller| {
            ledger_controller
                .expect_get_datastore_keys()
                .returning(move |_, _, _, _, _| None);
        });

    foreign_controllers
        .final_state
        .write()
        .expect_get_ledger()
        .return_const(Box::new(foreign_controllers.ledger_controller.clone()));

    let mut rng = thread_rng();

    // Generate random datastore entries
    let mut data = BTreeMap::new();
    for _ in 0..nb_keys {
        let key: Vec<u8> = (0..rng.gen_range(2..10))
            .map(|_| rng.sample(Alphanumeric) as u8)
            .collect();
        let value: Vec<u8> = (0..rng.gen_range(1..10))
            .map(|_| rng.sample(Alphanumeric) as u8)
            .collect();
        data.insert(key, value);
    }

    let original_data = data.clone(); // Keep original data for later comparison

    let mut changes = PreHashMap::default();

    changes.insert(
        addr,
        massa_models::types::SetUpdateOrDelete::Set(LedgerEntry {
            datastore: data.clone(),
            ..Default::default()
        }),
    );

    let exec_output = ExecutionOutput {
        slot: Slot::new(1, 0),
        block_info: None,
        state_changes: StateChanges {
            ledger_changes: LedgerChanges(changes.clone()),
            async_pool_changes: Default::default(),
            deferred_call_changes: Default::default(),
            pos_changes: Default::default(),
            executed_ops_changes: Default::default(),
            executed_denunciations_changes: Default::default(),
            execution_trail_hash_change: Default::default(),
        },
        events: Default::default(),
        #[cfg(feature = "execution-trace")]
        slot_trace: Default::default(),
        #[cfg(feature = "dump-block")]
        storage: None,
        deferred_credits_execution: Default::default(),
        cancel_async_message_execution: Default::default(),
        auto_sell_execution: Default::default(),
        transfers_history: Default::default(),
    };

    let mut active_history_entries = VecDeque::from([exec_output.clone()]);

    // Generate random updates and deletions using existing keys
    let mut update_changes = PreHashMap::default();
    let mut datastore_updates = BTreeMap::new();

    let existing_keys: Vec<_> = data.keys().cloned().collect();

    // Generate random updates and deletions
    for _ in 0..rng.gen_range(1..5) {
        if let Some(key) = existing_keys.choose(&mut rng) {
            let value: Vec<u8> = (0..rng.gen_range(1..10))
                .map(|_| rng.sample(Alphanumeric) as u8)
                .collect();
            datastore_updates.insert(key.clone(), massa_models::types::SetOrDelete::Set(value));
        }
    }

    for _ in 0..rng.gen_range(1..3) {
        if let Some(key) = existing_keys.choose(&mut rng) {
            datastore_updates.insert(key.clone(), massa_models::types::SetOrDelete::Delete);
        }
    }

    update_changes.insert(
        addr,
        massa_models::types::SetUpdateOrDelete::Update(LedgerEntryUpdate {
            datastore: datastore_updates.clone(),
            ..Default::default()
        }),
    );

    let exec_output_update = ExecutionOutput {
        slot: Slot::new(2, 0),
        block_info: None,
        state_changes: StateChanges {
            ledger_changes: LedgerChanges(update_changes.clone()),
            async_pool_changes: Default::default(),
            deferred_call_changes: Default::default(),
            pos_changes: Default::default(),
            executed_ops_changes: Default::default(),
            executed_denunciations_changes: Default::default(),
            execution_trail_hash_change: Default::default(),
        },
        events: Default::default(),
        #[cfg(feature = "execution-trace")]
        slot_trace: Default::default(),
        #[cfg(feature = "dump-block")]
        storage: None,
        deferred_credits_execution: Default::default(),
        cancel_async_message_execution: Default::default(),
        auto_sell_execution: Default::default(),
        transfers_history: Default::default(),
    };

    active_history_entries.push_back(exec_output_update);

    let active_history = Arc::new(RwLock::new(ActiveHistory(active_history_entries)));

    // Scan datastore with random bounds
    let start_key = if rng.gen_bool(0.5) {
        Bound::Included(existing_keys.choose(&mut rng).unwrap_or(&vec![]).clone())
    } else {
        Bound::Unbounded
    };

    let end_key = if rng.gen_bool(0.5) {
        Bound::Excluded(existing_keys.choose(&mut rng).unwrap_or(&vec![]).clone())
    } else {
        Bound::Unbounded
    };

    let (final_keys, candidate_keys) = scan_datastore(
        &addr,
        &[],
        start_key.clone(),
        end_key.clone(),
        None,
        foreign_controllers.final_state.clone(),
        active_history.clone(),
        None,
    );

    assert!(final_keys.is_none());

    let mut candidate_k = candidate_keys.unwrap();

    // Extract keys from the generated datastore and verify
    let mut expected_keys: Vec<_> = original_data
        .iter()
        .filter_map(|(key, _)| match datastore_updates.get(key) {
            Some(massa_models::types::SetOrDelete::Delete) => None,
            _ => Some(key.clone()),
        })
        .filter(|key| match &start_key {
            Bound::Included(sk) => key >= sk,
            Bound::Excluded(sk) => key > sk,
            Bound::Unbounded => true,
        })
        .filter(|key| match &end_key {
            Bound::Included(ub) => key <= ub,
            Bound::Excluded(ub) => key < ub,
            Bound::Unbounded => true,
        })
        .collect();
    expected_keys.sort();

    // println!("Expected keys:");
    // display_human_readable(expected_keys.clone());

    // println!("Candidate keys:");
    // display_human_readable(candidate_k.clone().into_iter().collect());

    for expected_key in expected_keys {
        assert_eq!(candidate_k.pop_first().unwrap(), expected_key);
    }
}

#[allow(dead_code)]
fn display_human_readable(keys: Vec<Vec<u8>>) {
    println!("Keys:");
    for key in keys {
        match std::str::from_utf8(&key) {
            Ok(key_str) => println!("  {}", key_str),
            Err(e) => panic!("{}", e.to_string()),
        }
    }
}

#[allow(dead_code)]
fn display_bound_human_readable(bound: Bound<Vec<u8>>) {
    match &bound {
        Bound::Included(b) => dbg!(format!(
            "bound key included : {}",
            String::from_utf8(b.clone()).unwrap()
        )),
        Bound::Excluded(b) => dbg!(format!(
            "bound key excluded : {}",
            String::from_utf8(b.clone()).unwrap()
        )),
        Bound::Unbounded => dbg!("bound key Unbounded".to_string()),
    };
}
