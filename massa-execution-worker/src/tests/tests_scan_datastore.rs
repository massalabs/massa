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
