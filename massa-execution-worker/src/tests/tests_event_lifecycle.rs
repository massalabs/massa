// Copyright (c) 2026 MASSA LABS <info@massa.net>

use super::universe::ExecutionTestUniverse;
use crate::execution::ExecutionState;
use massa_async_pool::{AsyncPool, AsyncPoolChanges, AsyncPoolConfig};
use massa_db_exports::{DBBatch, MassaDBConfig, MassaDBController, ShareableMassaDBController};
use massa_db_worker::MassaDB;
use massa_deferred_calls::{
    config::DeferredCallsConfig, registry_changes::DeferredCallRegistryChanges,
    slot_changes::DeferredRegistrySlotChanges, DeferredCall, DeferredCallRegistry,
};
use massa_event_cache::MockEventCacheControllerWrapper;
use massa_execution_exports::{
    EventStore, ExecutionBlockMetadata, ExecutionChannels, ExecutionConfig, ExecutionOutput,
    SlotExecutionOutput,
};
use massa_final_state::{test_exports::get_sample_state, FinalStateController};
use massa_ledger_exports::{LedgerChanges, LedgerEntry};
use massa_metrics::MassaMetrics;
use massa_models::{
    address::Address,
    amount::Amount,
    async_msg::AsyncMessage,
    async_msg_id::AsyncMessageId,
    bytecode::Bytecode,
    config::{MIP_STORE_STATS_BLOCK_CONSIDERED, THREAD_COUNT},
    deferred_calls::DeferredCallId,
    operation::SecureShareOperation,
    output_event::SCOutputEvent,
    prehash::PreHashMap,
    slot::Slot,
    types::{SetOrKeep, SetUpdateOrDelete},
};
use massa_pos_exports::MockSelectorControllerWrapper;
use massa_proto_rs::massa::model::v1::{ExecutionOutputStatus, ScExecutionEventStatus};
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_test_framework::TestUniverse;
use massa_versioning::{
    mips::get_mip_list,
    versioning::{MipStatsConfig, MipStore},
};
use massa_wallet::test_exports::create_test_wallet;
use num::rational::Ratio;
use parking_lot::RwLock;
use std::{collections::BTreeMap, str::FromStr, sync::Arc, time::Duration};
use tempfile::{NamedTempFile, TempDir};
use tokio::sync::broadcast;

#[derive(Clone, Copy, Debug)]
enum DeferredProfile {
    Success,
    VmFailure,
    ValidationFailure,
    Cancelled,
}

struct Fixture {
    state: ExecutionState,
    final_state: Arc<RwLock<dyn FinalStateController>>,
    db: ShareableMassaDBController,
    saved: Arc<RwLock<EventStore>>,
    receiver: broadcast::Receiver<SlotExecutionOutput>,
    _db_dir: TempDir,
    _cache_dir: TempDir,
    _ledger_file: NamedTempFile,
}

fn selector() -> Box<MockSelectorControllerWrapper> {
    let mut selector = MockSelectorControllerWrapper::new();
    selector.set_expectations(|mock| {
        mock.expect_feed_cycle().returning(|_, _, _| Ok(()));
        mock.expect_wait_for_draws().returning(Ok);
        mock.expect_get_producer().returning(|_| {
            Ok(Address::from_public_key(
                &KeyPair::from_str("S1FpYC4ugG9ivZZbLVrTwWtF9diSRiAwwrVX5Gx1ANSRLfouUjq")
                    .unwrap()
                    .get_public_key(),
            ))
        });
    });
    Box::new(selector)
}

fn fixture(broadcast_enabled: bool) -> Fixture {
    let db_dir = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let db: ShareableMassaDBController =
        Arc::new(RwLock::new(Box::new(MassaDB::new(MassaDBConfig {
            path: db_dir.path().to_path_buf(),
            max_history_length: 10,
            max_final_state_elements_size: 100_000,
            max_versioning_elements_size: 100_000,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 10,
            enable_metrics: false,
        })) as Box<dyn MassaDBController>));
    let mip_store = MipStore::try_from((
        get_mip_list(),
        MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            warn_announced_version_ratio: Ratio::new_raw(30, 100),
        },
    ))
    .unwrap();
    let (final_state, ledger_file) =
        get_sample_state(0, selector(), mip_store.clone(), db.clone()).unwrap();
    let config = ExecutionConfig {
        broadcast_enabled,
        hd_cache_path: cache_dir.path().join("modules"),
        #[cfg(feature = "dump-block")]
        block_dump_folder_path: cache_dir.path().join("blocks"),
        ..Default::default()
    };
    let (slot_execution_output_sender, receiver) = broadcast::channel(16);
    let channels = ExecutionChannels {
        slot_execution_output_sender,
        #[cfg(feature = "execution-trace")]
        slot_execution_traces_sender: broadcast::channel(16).0,
        #[cfg(feature = "execution-info")]
        slot_execution_info_sender: broadcast::channel(16).0,
    };
    let saved = Arc::new(RwLock::new(EventStore::default()));
    let events = saved.clone();
    let mut event_cache = MockEventCacheControllerWrapper::new();
    event_cache.set_expectations(move |mock| {
        mock.expect_save_events()
            .returning(move |new| events.write().0.extend(new));
    });
    let state = ExecutionState::new(
        config.clone(),
        final_state.clone(),
        mip_store,
        selector(),
        channels,
        Arc::new(RwLock::new(create_test_wallet(Some(PreHashMap::default())))),
        MassaMetrics::new(
            false,
            "127.0.0.1:0".parse().unwrap(),
            THREAD_COUNT,
            Duration::from_secs(1),
        )
        .0,
        Box::new(event_cache),
        #[cfg(all(feature = "dump-block", feature = "db_storage_backend"))]
        Arc::new(RwLock::new(
            crate::storage_backend::RocksDBStorageBackend::new(
                config.block_dump_folder_path.clone(),
                10,
            ),
        )),
        #[cfg(all(
            feature = "dump-block",
            feature = "file_storage_backend",
            not(feature = "db_storage_backend")
        ))]
        Arc::new(RwLock::new(
            crate::storage_backend::FileStorageBackend::new(
                config.block_dump_folder_path.clone(),
                10,
            ),
        )),
    );
    Fixture {
        state,
        final_state,
        db,
        saved,
        receiver,
        _db_dir: db_dir,
        _cache_dir: cache_dir,
        _ledger_file: ledger_file,
    }
}

struct Workload {
    pub target: Address,
    pub operation: SecureShareOperation,
    pub deferred: DeferredCallId,
    pub message: AsyncMessageId,
}
fn seed(
    final_state: &Arc<RwLock<dyn FinalStateController>>,
    db: ShareableMassaDBController,
    profile: DeferredProfile,
) -> Workload {
    let keypair = KeyPair::from_str("S1FpYC4ugG9ivZZbLVrTwWtF9diSRiAwwrVX5Gx1ANSRLfouUjq").unwrap();
    let sender = Address::from_public_key(&keypair.get_public_key());
    assert_eq!(sender.get_thread(32), 1);
    let target =
        Address::from_str("AS12jc7fTsSKwQ9hSk97C3iMNgNT1XrrD6MjSJRJZ4NE53YgQ4kFV").unwrap();
    let slot = Slot::new(1, 1);
    let mut batch = DBBatch::new();
    let entry = LedgerEntry {
        balance: Amount::from_raw(100_000_000_000),
        bytecode: Bytecode(include_bytes!("./wasm/receive_message.wasm").to_vec()),
        ..Default::default()
    };
    final_state.write().get_ledger_mut().apply_changes_to_batch(
        LedgerChanges(PreHashMap::from_iter([(
            target,
            SetUpdateOrDelete::Set(entry),
        )])),
        &mut batch,
    );
    let async_message = AsyncMessage {
        emission_slot: Slot::new(0, 31),
        emission_index: 0,
        sender,
        destination: target,
        function: "receive".into(),
        max_gas: 3_000_000,
        fee: Amount::from_raw(1),
        coins: Amount::from_raw(100),
        validity_start: slot,
        validity_end: Slot::new(2, 0),
        function_params: vec![42; 4],
        trigger: None,
        can_be_executed: true,
    };
    let message = async_message.compute_id();
    let mut pool = AsyncPool::new(AsyncPoolConfig::default(), db.clone());
    pool.apply_changes_to_batch(
        &AsyncPoolChanges(BTreeMap::from([(
            message,
            SetUpdateOrDelete::Set(async_message),
        )])),
        &mut batch,
    );
    let mut call = DeferredCall {
        sender_address: sender,
        target_slot: slot,
        target_address: target,
        target_function: "receive".into(),
        parameters: vec![42; 4],
        coins: Amount::from_raw(100),
        max_gas: 2_300_000,
        fee: Amount::from_raw(1),
        cancelled: false,
    };
    match profile {
        DeferredProfile::Success => (),
        DeferredProfile::VmFailure => call.target_function = "not_exported".into(),
        DeferredProfile::ValidationFailure => call.target_address = sender,
        DeferredProfile::Cancelled => call.cancelled = true,
    }
    let effective_gas = if call.cancelled { 0 } else { 2_300_000 };
    let deferred = DeferredCallId::new(0, slot, 0, b"e3-private-workload").unwrap();
    let mut changes = DeferredRegistrySlotChanges::default();
    changes.set_call(deferred.clone(), call);
    changes.set_effective_slot_gas(effective_gas);
    DeferredCallRegistry::new(db.clone(), DeferredCallsConfig::default()).apply_changes_to_batch(
        DeferredCallRegistryChanges {
            slots_change: BTreeMap::from([(slot, changes)]),
            effective_total_gas: SetOrKeep::Set(u128::from(effective_gas)),
            exec_stats: (0, 0, 0),
        },
        &mut batch,
    );
    db.write().write_batch(batch, Default::default(), None);
    final_state.write().recompute_caches();
    assert!(final_state
        .read()
        .get_async_pool()
        .message_cache
        .contains_key(&message));
    assert!(final_state
        .read()
        .get_deferred_call_registry()
        .get_call(&slot, &deferred)
        .is_some());
    let operation = crate::tests::universe::ExecutionTestUniverse::create_call_sc_operation(
        &keypair,
        3_000_000,
        Amount::from_raw(10_000_000),
        Amount::from_raw(100),
        target,
        "receive".into(),
        vec![42; 4],
    )
    .unwrap();
    Workload {
        target,
        operation,
        deferred,
        message,
    }
}

struct Outputs {
    workload: Workload,
    candidate: ExecutionOutput,
    final_events: Vec<SCOutputEvent>,
    broadcast: Option<ExecutionOutput>,
}

fn execute(profile: DeferredProfile, cached: bool, broadcast_enabled: bool) -> Outputs {
    let mut f = fixture(broadcast_enabled);
    let w = seed(&f.final_state, f.db.clone(), profile);
    f.state
        .execute_final_slot(&Slot::new(1, 0), None, selector());
    while f.receiver.try_recv().is_ok() {}
    f.saved.write().clear();
    let slot = Slot::new(1, 1);
    let key = KeyPair::from_str("S1FpYC4ugG9ivZZbLVrTwWtF9diSRiAwwrVX5Gx1ANSRLfouUjq").unwrap();
    let block =
        ExecutionTestUniverse::create_block(&key, slot, vec![w.operation.clone()], vec![], vec![]);
    let mut storage = Storage::create_root();
    storage.store_operations(vec![w.operation.clone()]);
    storage.store_block(block.clone());
    let target = (
        block.id,
        ExecutionBlockMetadata {
            same_thread_parent_creator: Some(Address::from_public_key(&key.get_public_key())),
            storage: Some(storage),
        },
    );
    f.state
        .execute_candidate_slot(&slot, Some(&target), selector());
    let candidate = f.state.active_history.read().0.back().unwrap().clone();
    assert!(!candidate.events.0.is_empty());
    assert!(candidate.events.0.iter().all(|e| !e.context.is_final));
    if broadcast_enabled {
        match f.receiver.try_recv().unwrap() {
            SlotExecutionOutput::ExecutedSlot(output) => {
                assert_eq!(output.events.0, candidate.events.0)
            }
            _ => panic!("candidate output required"),
        }
    } else {
        assert!(matches!(
            f.receiver.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }
    let received = if matches!(profile, DeferredProfile::Success) {
        300
    } else {
        200
    };
    assert_eq!(
        f.state.get_final_and_candidate_balance(&w.target),
        (
            Some(Amount::from_raw(100_000_000_000)),
            Some(Amount::from_raw(100_000_000_000 + received))
        )
    );
    if !cached {
        f.state.active_history.write().0.clear();
    }
    f.state.execute_final_slot(&slot, Some(&target), selector());
    assert_eq!(f.state.final_cursor, slot);
    let state = f.final_state.read();
    assert_eq!(
        state.get_ops_exec_status(&[w.operation.id]),
        vec![Some(true)]
    );
    assert_eq!(
        state.get_ledger().get_balance(&w.target),
        Some(Amount::from_raw(100_000_000_000 + received))
    );
    assert!(state
        .get_deferred_call_registry()
        .get_call(&slot, &w.deferred)
        .is_none());
    let mut pool = AsyncPool::new(AsyncPoolConfig::default(), f.db.clone());
    pool.recompute_message_cache();
    assert!(
        pool.message_cache.is_empty(),
        "persistent async message cleanup"
    );
    assert_eq!(
        state.get_async_pool().fetch_messages(&[w.message]),
        vec![(w.message, None)]
    );
    let final_events: Vec<_> = f.saved.read().0.iter().cloned().collect();
    let mut expected = candidate.events.clone();
    expected.finalize();
    assert_eq!(final_events, expected.0.into_iter().collect::<Vec<_>>());
    let broadcast = if broadcast_enabled {
        if !cached {
            match f.receiver.try_recv().unwrap() {
                SlotExecutionOutput::ExecutedSlot(output) => {
                    assert_eq!(output.events.0, candidate.events.0);
                }
                _ => panic!("fresh execution must first publish its candidate output"),
            }
        }
        match f.receiver.try_recv().unwrap() {
            SlotExecutionOutput::FinalizedSlot(output) => Some(output),
            _ => panic!("final output required"),
        }
    } else {
        assert!(matches!(
            f.receiver.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
        None
    };
    println!(
        "EVENT_LIFECYCLE {}",
        serde_json::json!({"profile":format!("{profile:?}"),"cached":cached,"broadcast_enabled":broadcast_enabled,"candidate":candidate.events.0,"final_events":final_events,"broadcast_events":broadcast.as_ref().map(|o| &o.events.0),"operation_id":w.operation.id.to_string(),"deferred_id":w.deferred.to_string(),"message_id":format!("{:?}",w.message),"target_balance_raw":100_000_000_000u64+received,"operation_success":true,"persistent_cleanup":true})
    );
    Outputs {
        workload: w,
        candidate,
        final_events,
        broadcast,
    }
}

#[test]
fn final_event_status_cached_and_fresh_finalization() {
    for cached in [true, false] {
        for profile in [DeferredProfile::Success, DeferredProfile::VmFailure] {
            let output = execute(profile, cached, true);
            let broadcast = output.broadcast.unwrap();
            assert!(
                broadcast.events.0.iter().all(|e| e.context.is_final),
                "final broadcast events must be finalized"
            );
            let proto: massa_proto_rs::massa::model::v1::SlotExecutionOutput =
                SlotExecutionOutput::FinalizedSlot(broadcast.clone()).into();
            assert_eq!(
                broadcast.events.0.into_iter().collect::<Vec<_>>(),
                output.final_events
            );
            assert_eq!(proto.status, ExecutionOutputStatus::Final as i32);
            assert!(proto.execution_output.unwrap().events.iter().all(|e| e
                .context
                .as_ref()
                .unwrap()
                .status
                == ScExecutionEventStatus::Final as i32));
            assert!(output
                .candidate
                .events
                .0
                .iter()
                .all(|e| !e.context.is_final));
        }
    }
}

#[test]
fn final_event_status_broadcast_disabled() {
    let output = execute(DeferredProfile::VmFailure, true, false);
    assert!(output.broadcast.is_none());
    assert!(output.final_events.iter().all(|e| e.context.is_final));
    assert!(output.final_events.iter().any(|e| e.context.is_error));
}

fn check_attribution(output: Outputs, profile: DeferredProfile) {
    let w = &output.workload;
    for events in [
        output.candidate.events.0.into_iter().collect::<Vec<_>>(),
        output.final_events,
    ] {
        let deferred: Vec<_> = events
            .iter()
            .filter(|e| e.context.deferred_call_id.as_ref() == Some(&w.deferred))
            .collect();
        assert_eq!(
            deferred.len(),
            usize::from(!matches!(profile, DeferredProfile::Cancelled)),
            "deferred origin must not leak"
        );
        for event in deferred {
            assert_eq!(event.context.origin_operation_id, None);
            assert_eq!(event.context.async_msg_id, None);
            assert_eq!(
                event.context.is_error,
                !matches!(profile, DeferredProfile::Success)
            );
        }
        let op: Vec<_> = events
            .iter()
            .filter(|e| e.context.origin_operation_id == Some(w.operation.id))
            .collect();
        let asc: Vec<_> = events
            .iter()
            .filter(|e| e.context.async_msg_id == Some(w.message))
            .collect();
        assert_eq!(op.len(), 1);
        assert_eq!(asc.len(), 1);
        for event in op.into_iter().chain(asc) {
            assert_eq!(
                event.context.deferred_call_id, None,
                "ordinary event must not inherit deferred origin"
            );
            assert!(!event.context.is_error);
            assert_eq!(event.data, "message correctly received: 42,42,42,42");
            assert_eq!(event.context.call_stack.back(), Some(&w.target));
        }
        assert_eq!(
            events
                .iter()
                .map(|e| e.context.index_in_slot)
                .collect::<Vec<_>>(),
            (0..events.len() as u64).collect::<Vec<_>>()
        );
    }
}

#[test]
fn deferred_attribution_same_slot_sequence() {
    check_attribution(
        execute(DeferredProfile::Success, true, true),
        DeferredProfile::Success,
    );
}

#[test]
fn deferred_attribution_failure_and_cancellation() {
    for profile in [
        DeferredProfile::ValidationFailure,
        DeferredProfile::VmFailure,
        DeferredProfile::Cancelled,
    ] {
        check_attribution(execute(profile, true, true), profile);
    }
}
