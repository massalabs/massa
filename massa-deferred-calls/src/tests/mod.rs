use super::*;
use crate::{DeferredCall, DeferredCallRegistry, DeferredRegistryChanges};
use massa_db_exports::{DBBatch, MassaDBConfig, MassaDBController, ShareableMassaDBController};
use massa_db_worker::MassaDB;
use massa_models::{
    address::Address,
    amount::Amount,
    config::THREAD_COUNT,
    deferred_call_id::{DeferredCallId, DeferredCallIdSerializer},
    slot::Slot,
};
use parking_lot::RwLock;
use std::{str::FromStr, sync::Arc};
use tempfile::tempdir;

#[test]
fn call_registry_apply_changes() {
    let temp_dir = tempdir().expect("Unable to create a temp folder");
    let db_config = MassaDBConfig {
        path: temp_dir.path().to_path_buf(),
        max_history_length: 100,
        max_final_state_elements_size: 100,
        max_versioning_elements_size: 100,
        thread_count: THREAD_COUNT,
        max_ledger_backups: 100,
    };
    let call_id_serializer = DeferredCallIdSerializer::new();
    let db: ShareableMassaDBController = Arc::new(RwLock::new(
        Box::new(MassaDB::new(db_config)) as Box<(dyn MassaDBController + 'static)>
    ));

    let registry = DeferredCallRegistry::new(db);

    let mut changes = DeferredRegistryChanges::default();

    let target_slot = Slot {
        thread: 5,
        period: 1,
    };

    let call = DeferredCall::new(
        Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
        target_slot.clone(),
        Address::from_str("AS127QtY6Hzm6BnJc9wqCBfPNvEH9fKer3LiMNNQmcX3MzLwCL6G6").unwrap(),
        "receive".to_string(),
        vec![42, 42, 42, 42],
        Amount::from_raw(100),
        3000000,
        Amount::from_raw(1),
        false,
    );
    let id = DeferredCallId::new(0, target_slot.clone(), 1, &[]).unwrap();
    let mut buf_id = Vec::new();
    call_id_serializer.serialize(&id, &mut buf_id).unwrap();

    changes.set_call(id.clone(), call.clone());

    let mut batch = DBBatch::new();
    registry.apply_changes_to_batch(changes, &mut batch);

    registry.db.write().write_batch(batch, DBBatch::new(), None);

    let result = registry.get_call(&target_slot, &id).unwrap();
    assert!(result.target_function.eq(&call.target_function));
    assert_eq!(result.sender_address, call.sender_address);
}

#[test]
fn call_registry_get_slot_calls() {
    let temp_dir = tempdir().expect("Unable to create a temp folder");
    let db_config = MassaDBConfig {
        path: temp_dir.path().to_path_buf(),
        max_history_length: 100,
        max_final_state_elements_size: 100,
        max_versioning_elements_size: 100,
        thread_count: THREAD_COUNT,
        max_ledger_backups: 100,
    };
    let call_id_serializer = DeferredCallIdSerializer::new();
    let db: ShareableMassaDBController = Arc::new(RwLock::new(
        Box::new(MassaDB::new(db_config)) as Box<(dyn MassaDBController + 'static)>
    ));

    let registry = DeferredCallRegistry::new(db);

    let mut changes = DeferredRegistryChanges::default();

    let target_slot = Slot {
        thread: 5,
        period: 1,
    };

    let call = DeferredCall::new(
        Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
        target_slot.clone(),
        Address::from_str("AS127QtY6Hzm6BnJc9wqCBfPNvEH9fKer3LiMNNQmcX3MzLwCL6G6").unwrap(),
        "receive".to_string(),
        vec![42, 42, 42, 42],
        Amount::from_raw(100),
        3000000,
        Amount::from_raw(1),
        false,
    );
    let id = DeferredCallId::new(0, target_slot.clone(), 1, &[]).unwrap();

    let id2 = DeferredCallId::new(0, target_slot.clone(), 1, &[123]).unwrap();

    let mut buf_id = Vec::new();
    call_id_serializer.serialize(&id, &mut buf_id).unwrap();

    changes.set_call(id.clone(), call.clone());
    changes.set_call(id2.clone(), call.clone());
    changes.set_total_gas(100);
    changes.set_slot_gas(target_slot.clone(), 100_000);

    changes.set_slot_base_fee(target_slot.clone(), Amount::from_raw(10000000));

    let mut batch = DBBatch::new();
    // 2 calls
    registry.apply_changes_to_batch(changes, &mut batch);

    registry.db.write().write_batch(batch, DBBatch::new(), None);

    let result = registry.get_slot_calls(target_slot.clone());

    assert!(result.slot_calls.len() == 2);
    assert!(result.slot_calls.contains_key(&id));
    assert!(result.slot_calls.contains_key(&id2));
    assert_eq!(result.total_gas, 100);
    assert_eq!(result.slot_base_fee, Amount::from_raw(10000000));
    assert_eq!(result.slot_gas, 100_000);
}
