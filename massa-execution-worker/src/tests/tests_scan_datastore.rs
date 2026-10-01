use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
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

use crate::{
    active_history::ActiveHistory,
    datastore_scan::{scan_datastore, MIN_FINAL_KEYS_REFILL},
};

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
        execution_info: None,
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
        execution_info: None,
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
        execution_info: None,
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

/// Tripwire for the unbounded per-item scan: with `count = None`, the scan returns
/// every key with no cap at all. This documents the hole left while #5189 (per-request
/// count forwarding) is unmerged: a single datastore-keys item can pull the full range
/// under the execution lock. When #5189 lands, this test breaks on purpose — update it
/// to assert the capped count instead. See issue #5057, phase 2.
#[test]
fn test_scan_datastore_count_none_is_unbounded() {
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

    // 2000 deterministic keys, above any per-request cap
    let mut data = BTreeMap::new();
    for i in 0..2000usize {
        data.insert(format!("key{:05}", i).into_bytes(), b"v".to_vec());
    }

    let mut changes = PreHashMap::default();
    changes.insert(
        addr,
        massa_models::types::SetUpdateOrDelete::Set(LedgerEntry {
            datastore: data,
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
        execution_info: None,
    };

    let active_history = Arc::new(RwLock::new(ActiveHistory(VecDeque::from([exec_output]))));

    let (_final_keys, candidate_keys) = scan_datastore(
        &addr,
        &[],
        Bound::Unbounded,
        Bound::Unbounded,
        None,
        foreign_controllers.final_state.clone(),
        active_history,
        None,
    );

    // No cap applied: all 2000 keys come back in one item.
    assert_eq!(candidate_keys.unwrap().len(), 2000);
}

/// Builds foreign controllers whose final ledger exposes no datastore keys
/// (all keys come from the speculative history).
fn controllers_without_final_keys() -> ExecutionForeignControllers {
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
    foreign_controllers
}

/// Builds foreign controllers whose final ledger returns the given keys, honouring
/// the queried start/end bounds and count (mirroring the ledger contract).
fn controllers_with_final_keys(keys: Vec<Vec<u8>>) -> ExecutionForeignControllers {
    controllers_with_final_keys_recording(keys, Default::default())
}

/// Same as [`controllers_with_final_keys`], also recording the `count` of every final-ledger
/// query, in order.
fn controllers_with_final_keys_recording(
    keys: Vec<Vec<u8>>,
    requested_counts: Arc<std::sync::Mutex<Vec<Option<u32>>>>,
) -> ExecutionForeignControllers {
    let mut foreign_controllers = ExecutionForeignControllers::new_with_mocks();
    foreign_controllers
        .ledger_controller
        .set_expectations(move |ledger_controller| {
            let requested_counts = requested_counts.clone();
            let keys = keys.clone();
            ledger_controller.expect_get_datastore_keys().returning(
                move |_addr, _prefix, start_key, end_key, count| {
                    requested_counts.lock().unwrap().push(count);
                    let mut out: BTreeSet<Vec<u8>> = keys
                        .iter()
                        .filter(|&k| match &start_key {
                            Bound::Included(sk) => k >= sk,
                            Bound::Excluded(sk) => k > sk,
                            Bound::Unbounded => true,
                        })
                        .filter(|&k| match &end_key {
                            Bound::Included(ek) => k <= ek,
                            Bound::Excluded(ek) => k < ek,
                            Bound::Unbounded => true,
                        })
                        .cloned()
                        .collect();
                    if let Some(cnt) = count {
                        out = out.into_iter().take(cnt as usize).collect();
                    }
                    Some(out)
                },
            );
        });
    foreign_controllers
        .final_state
        .write()
        .expect_get_ledger()
        .return_const(Box::new(foreign_controllers.ledger_controller.clone()));
    foreign_controllers
}

fn exec_output_with_changes(slot: Slot, ledger_changes: LedgerChanges) -> ExecutionOutput {
    ExecutionOutput {
        slot,
        block_info: None,
        state_changes: StateChanges {
            ledger_changes,
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
        execution_info: None,
    }
}

/// Builds an execution output carrying a full ledger-entry `Set` for `addr`.
fn set_output(slot: Slot, addr: Address, datastore: BTreeMap<Vec<u8>, Vec<u8>>) -> ExecutionOutput {
    let mut changes = PreHashMap::default();
    changes.insert(
        addr,
        massa_models::types::SetUpdateOrDelete::Set(LedgerEntry {
            datastore,
            ..Default::default()
        }),
    );
    exec_output_with_changes(slot, LedgerChanges(changes))
}

/// Builds an execution output carrying a ledger-entry `Update` for `addr`.
fn update_output(
    slot: Slot,
    addr: Address,
    datastore: BTreeMap<Vec<u8>, massa_models::types::SetOrDelete<Vec<u8>>>,
) -> ExecutionOutput {
    let mut changes = PreHashMap::default();
    changes.insert(
        addr,
        massa_models::types::SetUpdateOrDelete::Update(LedgerEntryUpdate {
            datastore,
            ..Default::default()
        }),
    );
    exec_output_with_changes(slot, LedgerChanges(changes))
}

/// A `count`ed query over a datastore much larger than `count` returns exactly the
/// first `count` keys, without copying the whole range.
#[test]
fn test_scan_datastore_set_branch_is_count_bounded() {
    let keypair = KeyPair::generate(0).unwrap();
    let addr = Address::from_public_key(&keypair.get_public_key());

    let foreign_controllers = controllers_without_final_keys();

    let data: BTreeMap<Vec<u8>, Vec<u8>> = (0..2000usize)
        .map(|i| (format!("key{:05}", i).into_bytes(), b"v".to_vec()))
        .collect();

    let active_history = Arc::new(RwLock::new(ActiveHistory(VecDeque::from([set_output(
        Slot::new(1, 0),
        addr,
        data,
    )]))));

    let (_final_keys, candidate_keys) = scan_datastore(
        &addr,
        &[],
        Bound::Unbounded,
        Bound::Unbounded,
        Some(10),
        foreign_controllers.final_state.clone(),
        active_history,
        None,
    );

    let expected: BTreeSet<Vec<u8>> = (0..10usize)
        .map(|i| format!("key{:05}", i).into_bytes())
        .collect();
    assert_eq!(candidate_keys.unwrap(), expected);
}

/// Regression for the ordered-merge bound: a newer update deleting one of the first
/// `count` keys must be replaced by the next key, not silently drop the result.
#[test]
fn test_scan_datastore_set_then_newer_delete_keeps_count() {
    let keypair = KeyPair::generate(0).unwrap();
    let addr = Address::from_public_key(&keypair.get_public_key());

    let foreign_controllers = controllers_without_final_keys();

    let mut data = BTreeMap::new();
    data.insert(b"a".to_vec(), b"va".to_vec());
    data.insert(b"b".to_vec(), b"vb".to_vec());
    data.insert(b"c".to_vec(), b"vc".to_vec());

    let mut deletion = BTreeMap::new();
    deletion.insert(b"a".to_vec(), massa_models::types::SetOrDelete::Delete);

    // oldest first: a full Set, then a newer Update deleting "a"
    let active_history = Arc::new(RwLock::new(ActiveHistory(VecDeque::from([
        set_output(Slot::new(1, 0), addr, data),
        update_output(Slot::new(2, 0), addr, deletion),
    ]))));

    let (_final_keys, candidate_keys) = scan_datastore(
        &addr,
        &[],
        Bound::Unbounded,
        Bound::Unbounded,
        Some(2),
        foreign_controllers.final_state.clone(),
        active_history,
        None,
    );

    // "a" is deleted, so the first two surviving keys are "b" and "c"
    let expected: BTreeSet<Vec<u8>> = [b"b".to_vec(), b"c".to_vec()].into_iter().collect();
    assert_eq!(candidate_keys.unwrap(), expected);
}

/// The merge branch must not drop deletions: a final key deleted in the speculative
/// history is absent from the result even with a small `count`, and the missing slot
/// is filled from the remaining final keys.
#[test]
fn test_scan_datastore_merge_keeps_delete_and_fills_count() {
    let keypair = KeyPair::generate(0).unwrap();
    let addr = Address::from_public_key(&keypair.get_public_key());

    let final_keys = vec![b"1".to_vec(), b"2".to_vec(), b"3".to_vec()];
    let foreign_controllers = controllers_with_final_keys(final_keys);

    let mut deletion = BTreeMap::new();
    deletion.insert(b"2".to_vec(), massa_models::types::SetOrDelete::Delete);

    let active_history = Arc::new(RwLock::new(ActiveHistory(VecDeque::from([update_output(
        Slot::new(1, 0),
        addr,
        deletion,
    )]))));

    let (_final_keys, candidate_keys) = scan_datastore(
        &addr,
        &[],
        Bound::Unbounded,
        Bound::Unbounded,
        Some(2),
        foreign_controllers.final_state.clone(),
        active_history,
        None,
    );

    // "2" is deleted; "1" and "3" fill the two requested keys
    let expected: BTreeSet<Vec<u8>> = [b"1".to_vec(), b"3".to_vec()].into_iter().collect();
    assert_eq!(candidate_keys.unwrap(), expected);
}

/// When many final keys ahead are speculatively deleted, refilling the final-key queue must
/// not degrade to one single-key ledger query per deleted key: each refill fetches at least
/// MIN_FINAL_KEYS_REFILL keys. The result is still exactly the first live key.
#[test]
fn test_scan_datastore_refill_is_batched_past_deleted_final_keys() {
    let keypair = KeyPair::generate(0).unwrap();
    let addr = Address::from_public_key(&keypair.get_public_key());

    let nb_final = 300usize;
    let nb_deleted = 200usize;
    let key = |i: usize| format!("k{:04}", i).into_bytes();

    let requested_counts: Arc<std::sync::Mutex<Vec<Option<u32>>>> = Default::default();
    let foreign_controllers = controllers_with_final_keys_recording(
        (0..nb_final).map(key).collect(),
        requested_counts.clone(),
    );

    // the first `nb_deleted` final keys are deleted in the speculative history
    let deletions: BTreeMap<_, _> = (0..nb_deleted)
        .map(|i| (key(i), massa_models::types::SetOrDelete::Delete))
        .collect();
    let active_history = Arc::new(RwLock::new(ActiveHistory(VecDeque::from([update_output(
        Slot::new(1, 0),
        addr,
        deletions,
    )]))));

    let (_final_keys, candidate_keys) = scan_datastore(
        &addr,
        &[],
        Bound::Unbounded,
        Bound::Unbounded,
        Some(1),
        foreign_controllers.final_state.clone(),
        active_history,
        None,
    );

    let expected: BTreeSet<Vec<u8>> = [key(nb_deleted)].into_iter().collect();
    assert_eq!(candidate_keys.unwrap(), expected);

    let requested = requested_counts.lock().unwrap().clone();
    // the first query asks for `count`; every refill after it for at least the floor
    assert_eq!(requested.first(), Some(&Some(1)));
    assert!(
        requested[1..]
            .iter()
            .all(|c| c.is_some_and(|c| c >= MIN_FINAL_KEYS_REFILL)),
        "refills below the floor: {:?}",
        requested
    );
    // one query for the first key, then ceil(deleted / floor) refills to get past the
    // deleted keys: far from the one query per deleted key the refill used to make
    let max_queries = 1 + nb_deleted.div_ceil(MIN_FINAL_KEYS_REFILL as usize) + 1;
    assert!(
        requested.len() <= max_queries,
        "{} final-ledger queries for {} deleted keys (expected at most {})",
        requested.len(),
        nb_deleted,
        max_queries
    );
}

/// Builds an execution output carrying a full ledger-entry `Delete` for `addr`.
fn delete_output(slot: Slot, addr: Address) -> ExecutionOutput {
    let mut changes = PreHashMap::default();
    changes.insert(addr, massa_models::types::SetUpdateOrDelete::Delete);
    exec_output_with_changes(slot, LedgerChanges(changes))
}

/// Keys set by updates following a full entry delete (oldest first).
fn updates_after_delete(addr: Address) -> ActiveHistory {
    let mut sets = BTreeMap::new();
    sets.insert(
        b"x".to_vec(),
        massa_models::types::SetOrDelete::Set(b"vx".to_vec()),
    );
    sets.insert(
        b"y".to_vec(),
        massa_models::types::SetOrDelete::Set(b"vy".to_vec()),
    );
    // a full entry delete, then updates: promoted to `Set` with no absolute
    // datastore (updates-only source)
    ActiveHistory(VecDeque::from([
        delete_output(Slot::new(1, 0), addr),
        update_output(Slot::new(2, 0), addr, sets),
    ]))
}

/// Regression (review #5286): with `count == Some(0)`, the updates-only `Set`
/// path (reset after a speculative delete) must return no keys — like the old
/// `take(0)` code and every other branch — instead of leaking one key.
#[test]
fn test_scan_datastore_reset_after_delete_count_zero_is_empty() {
    let keypair = KeyPair::generate(0).unwrap();
    let addr = Address::from_public_key(&keypair.get_public_key());

    let foreign_controllers = controllers_without_final_keys();
    let active_history = Arc::new(RwLock::new(updates_after_delete(addr)));

    let (_final_keys, candidate_keys) = scan_datastore(
        &addr,
        &[],
        Bound::Unbounded,
        Bound::Unbounded,
        Some(0),
        foreign_controllers.final_state.clone(),
        active_history,
        None,
    );

    assert_eq!(candidate_keys.unwrap(), BTreeSet::new());
}

/// The updates-only `Set` path (delete entry promoted by newer updates)
/// returns the updated keys with a normal `count`.
#[test]
fn test_scan_datastore_reset_after_delete_returns_updates() {
    let keypair = KeyPair::generate(0).unwrap();
    let addr = Address::from_public_key(&keypair.get_public_key());

    let foreign_controllers = controllers_without_final_keys();
    let active_history = Arc::new(RwLock::new(updates_after_delete(addr)));

    let (_final_keys, candidate_keys) = scan_datastore(
        &addr,
        &[],
        Bound::Unbounded,
        Bound::Unbounded,
        Some(10),
        foreign_controllers.final_state.clone(),
        active_history,
        None,
    );

    let expected: BTreeSet<Vec<u8>> = [b"x".to_vec(), b"y".to_vec()].into_iter().collect();
    assert_eq!(candidate_keys.unwrap(), expected);
}

/// Bounding the speculative scan must not change what is returned: for any `count`,
/// the result has to be exactly the first `count` keys of the unbounded result.
///
/// The reset (`Set`) path is the interesting one, because deletions applied on top of
/// the reset entry mean the scan has to look past the first `count` entry keys to
/// still produce `count` keys.
#[test]
fn test_scan_datastore_count_is_a_prefix_of_unbounded() {
    let mut rng = thread_rng();
    for _ in 0..20 {
        scan_datastore_count_prefix_case(rng.gen_range(20..60));
    }
}

fn scan_datastore_count_prefix_case(nb_keys: usize) {
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

    // the reset entry
    let mut data = BTreeMap::new();
    for i in 0..nb_keys {
        // fixed-width keys so that byte order matches the generation order
        let key = format!("key{:04}", i).into_bytes();
        let value: Vec<u8> = (0..rng.gen_range(1..10))
            .map(|_| rng.sample(Alphanumeric) as u8)
            .collect();
        data.insert(key, value);
    }
    let existing_keys: Vec<_> = data.keys().cloned().collect();

    let mut changes = PreHashMap::default();
    changes.insert(
        addr,
        massa_models::types::SetUpdateOrDelete::Set(LedgerEntry {
            datastore: data.clone(),
            ..Default::default()
        }),
    );

    // updates newer than the reset: deletions concentrated on the lowest keys, so a
    // naive `take(count)` on the entry would come up short, plus a few added keys
    let mut datastore_updates = BTreeMap::new();
    for key in existing_keys.iter().take(nb_keys / 3) {
        datastore_updates.insert(key.clone(), massa_models::types::SetOrDelete::Delete);
    }
    for i in 0..5 {
        // "add" sorts before "key", so these land at the front of the range
        datastore_updates.insert(
            format!("add{:04}", i).into_bytes(),
            massa_models::types::SetOrDelete::Set(vec![1, 2, 3]),
        );
    }

    let mut update_changes = PreHashMap::default();
    update_changes.insert(
        addr,
        massa_models::types::SetUpdateOrDelete::Update(LedgerEntryUpdate {
            datastore: datastore_updates.clone(),
            ..Default::default()
        }),
    );

    let mk_output = |slot, ledger_changes| ExecutionOutput {
        slot,
        block_info: None,
        state_changes: StateChanges {
            ledger_changes,
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
        execution_info: None,
    };

    let active_history = Arc::new(RwLock::new(ActiveHistory(VecDeque::from([
        mk_output(Slot::new(1, 0), LedgerChanges(changes)),
        mk_output(Slot::new(2, 0), LedgerChanges(update_changes)),
    ]))));

    let scan = |count| {
        scan_datastore(
            &addr,
            &[],
            Bound::Unbounded,
            Bound::Unbounded,
            count,
            foreign_controllers.final_state.clone(),
            active_history.clone(),
            None,
        )
        .1
        .expect("expected candidate keys")
    };

    let unbounded: Vec<_> = scan(None).into_iter().collect();

    // sanity: the deletions really do force the scan past the first keys of the entry
    assert!(unbounded.len() > nb_keys / 2);

    for count in 0..=(unbounded.len() + 2) {
        let bounded: Vec<_> = scan(Some(count as u32)).into_iter().collect();
        let expected: Vec<_> = unbounded.iter().take(count).cloned().collect();
        assert_eq!(
            bounded, expected,
            "count={} produced a different result than the unbounded scan",
            count
        );
    }
}
