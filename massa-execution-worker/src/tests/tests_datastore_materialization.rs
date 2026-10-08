// Copyright (c) 2026 MASSA LABS <info@massa.net>

//! Bounded characterization of production datastore reads and owned result buffers.
//! Native lookup counters describe current behavior, not a required API budget.
//! Retained Vec capacity does not measure transient allocations or process RSS.
//! Linux process snapshots include allocator, RocksDB, thread, and other process
//! memory; they do not attribute memory to a query. The write-lock contender
//! only observes admission on ExecutionState and does not execute consensus work.
//! VmHWM is the process-lifetime high-water mark, including fixture setup and
//! earlier work; RSS is not expected to fall after retained results are dropped.

use std::{
    collections::{BTreeMap, VecDeque},
    fs::File,
    io::Read,
    sync::{mpsc, Arc, Barrier},
    thread,
    time::Instant,
};

use massa_db_exports::{
    DBBatch, MassaDBConfig, MassaDBController, ShareableMassaDBController,
};
use massa_db_worker::MassaDB;
use massa_event_cache::MockEventCacheControllerWrapper;
use massa_execution_exports::{
    ExecutionChannels, ExecutionConfig, ExecutionController, ExecutionOutput,
};
use massa_final_state::{test_exports::get_sample_state, FinalStateController};
use massa_ledger_exports::{LedgerChanges, LedgerEntry, LedgerEntryUpdate};
use massa_metrics::MassaMetrics;
use massa_models::{
    address::Address,
    config::{MIP_STORE_STATS_BLOCK_CONSIDERED, THREAD_COUNT},
    prehash::PreHashMap,
    slot::Slot,
    types::{SetOrDelete, SetUpdateOrDelete},
};
use massa_pos_exports::MockSelectorControllerWrapper;
use massa_signature::KeyPair;
use massa_versioning::{
    mips::get_mip_list,
    versioning::{MipStatsConfig, MipStore},
};
use massa_wallet::test_exports::create_test_wallet;
use num::rational::Ratio;
use parking_lot::{Condvar, Mutex, RwLock};
use rocksdb::perf::{set_perf_stats, PerfContext, PerfMetric, PerfStatsLevel};
use tempfile::{NamedTempFile, TempDir};
use tokio::sync::broadcast;

use crate::{
    controller::{ExecutionControllerImpl, ExecutionInputData},
    execution::ExecutionState,
};

const VALUE_LEN: usize = 64 * 1024;
const MAX_BATCH_ITEMS: usize = 64;
const MAX_RESULT_BYTES: usize = 8 * 1024 * 1024;
const MAX_AGGREGATE_RESULT_BYTES: usize = 32 * 1024 * 1024;
const RESOURCE_BATCH_ITEMS: usize = 32;
const RESOURCE_BATCHES_PER_READER: usize = 8;
const RESOURCE_WRITE_ACQUISITIONS: usize = 32;
const PROCFS_READ_LIMIT: u64 = 64 * 1024;
const COORDINATION_TIMEOUT_SECS: u64 = 10;

struct ProductionDatastoreFixture {
    db: ShareableMassaDBController,
    final_state: Arc<RwLock<dyn FinalStateController>>,
    execution_state: Arc<RwLock<ExecutionState>>,
    controller: ExecutionControllerImpl,
    address_a: Address,
    address_b: Address,
    key: Vec<u8>,
    // Keep storage guards after all handles so they drop last.
    _db_dir: TempDir,
    _module_cache_dir: TempDir,
    _ledger_config_file: NamedTempFile,
}

impl ProductionDatastoreFixture {
    fn new() -> Self {
        let db_dir = TempDir::new().expect("create fixture database directory");
        let module_cache_dir =
            TempDir::new().expect("create fixture module cache directory");
        let db_config = MassaDBConfig {
            path: db_dir.path().to_path_buf(),
            max_history_length: 10,
            max_final_state_elements_size: 100_000,
            max_versioning_elements_size: 100_000,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 10,
            enable_metrics: false,
        };
        let db: ShareableMassaDBController =
            Arc::new(RwLock::new(Box::new(MassaDB::new(db_config))
                as Box<dyn MassaDBController + 'static>));

        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            warn_announced_version_ratio: Ratio::new_raw(30, 100),
        };
        let mip_store = MipStore::try_from((get_mip_list(), mip_stats_config))
            .expect("create fixture MIP store");
        let mut selector = MockSelectorControllerWrapper::new();
        selector.set_expectations(|mock| {
            mock.expect_feed_cycle().returning(|_, _, _| Ok(()));
            mock.expect_wait_for_draws().returning(|cycle| Ok(cycle));
        });

        let config = ExecutionConfig::default();
        let (final_state, ledger_config_file) = get_sample_state(
            config.last_start_period,
            Box::new(selector),
            mip_store.clone(),
            db.clone(),
        )
        .expect("create production sample final state");
        let address_a = Address::from_public_key(
            &KeyPair::generate(0)
                .expect("generate fixture address A")
                .get_public_key(),
        );
        let address_b = Address::from_public_key(
            &KeyPair::generate(0)
                .expect("generate fixture address B")
                .get_public_key(),
        );
        assert_ne!(address_a, address_b);
        let key = b"datastore-materialization".to_vec();
        let value_a = patterned_value(0x41);
        let value_b = patterned_value(0x42);
        insert_final_values(
            &final_state,
            db.clone(),
            &address_a,
            &address_b,
            &key,
            value_a,
            value_b,
        );

        let mut config = config;
        config.hd_cache_path = module_cache_dir.path().join("module-cache");
        #[cfg(feature = "dump-block")]
        {
            config.block_dump_folder_path =
                module_cache_dir.path().join("block-dump");
        }
        let (slot_execution_output_sender, _) = broadcast::channel(16);
        let channels = ExecutionChannels {
            slot_execution_output_sender,
            #[cfg(feature = "execution-trace")]
            slot_execution_traces_sender: broadcast::channel(16).0,
            #[cfg(feature = "execution-info")]
            slot_execution_info_sender: broadcast::channel(16).0,
        };
        let execution_state = Arc::new(RwLock::new(ExecutionState::new(
            config.clone(),
            final_state.clone(),
            mip_store,
            Box::new(MockSelectorControllerWrapper::new()),
            channels,
            Arc::new(RwLock::new(create_test_wallet(Some(
                PreHashMap::default(),
            )))),
            MassaMetrics::new(
                false,
                "127.0.0.1:0".parse().unwrap(),
                THREAD_COUNT,
                std::time::Duration::from_secs(1),
            )
            .0,
            Box::new(MockEventCacheControllerWrapper::new()),
            #[cfg(all(
                feature = "dump-block",
                feature = "db_storage_backend"
            ))]
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
        )));
        let input_data = Arc::new((
            Condvar::new(),
            Mutex::new(ExecutionInputData::new(config)),
        ));
        let controller = ExecutionControllerImpl {
            input_data,
            execution_state: execution_state.clone(),
        };

        Self {
            db,
            final_state,
            execution_state,
            controller,
            address_a,
            address_b,
            key,
            _db_dir: db_dir,
            _module_cache_dir: module_cache_dir,
            _ledger_config_file: ledger_config_file,
        }
    }
}

fn patterned_value(byte: u8) -> Vec<u8> {
    (0..VALUE_LEN).map(|i| byte.wrapping_add(i as u8)).collect()
}

fn insert_final_values(
    final_state: &Arc<RwLock<dyn FinalStateController>>,
    db: ShareableMassaDBController,
    address_a: &Address,
    address_b: &Address,
    key: &[u8],
    value_a: Vec<u8>,
    value_b: Vec<u8>,
) {
    let mut changes = PreHashMap::default();
    for (address, value) in [(address_a, value_a), (address_b, value_b)] {
        let mut datastore = BTreeMap::new();
        datastore.insert(key.to_vec(), value);
        changes.insert(
            *address,
            SetUpdateOrDelete::Set(LedgerEntry {
                datastore,
                ..Default::default()
            }),
        );
    }
    let mut batch = DBBatch::new();
    final_state
        .write()
        .get_ledger_mut()
        .apply_changes_to_batch(LedgerChanges(changes), &mut batch);
    db.write().write_batch(batch, Default::default(), None);
}

#[test]
fn datastore_backend_duplicate_values_materialize_independent_owned_buffers() {
    let fixture = ProductionDatastoreFixture::new();
    let expected_a = patterned_value(0x41);
    let expected_b = patterned_value(0x42);
    let mut perf = PerfContext::default();

    for batch_len in [1, 8, 32] {
        assert!(batch_len <= MAX_BATCH_ITEMS);
        assert!(batch_len * 2 * VALUE_LEN <= MAX_RESULT_BYTES);
        let input = vec![(fixture.address_a, fixture.key.clone()); batch_len];
        perf.reset();
        set_perf_stats(PerfStatsLevel::EnableCount);
        let started = Instant::now();
        let output = fixture.controller.get_final_and_active_data_entry(input);
        let elapsed = started.elapsed();
        set_perf_stats(PerfStatsLevel::Disable);
        let rocksdb_get_read_bytes = perf.metric(PerfMetric::GetReadBytes);
        let rocksdb_memtable_get_count =
            perf.metric(PerfMetric::GetFromMemtableCount);
        assert_eq!(rocksdb_get_read_bytes, (batch_len * VALUE_LEN) as u64);

        assert_eq!(output.len(), batch_len);
        let mut retained_payload_bytes = 0usize;
        let mut retained_capacity_bytes = 0usize;
        let mut buffers = Vec::with_capacity(batch_len * 2);
        for (final_value, active_value) in &output {
            let final_value = final_value.as_ref().expect("final value exists");
            let active_value =
                active_value.as_ref().expect("fallback active value exists");
            assert_eq!(final_value, &expected_a);
            assert_eq!(active_value, &expected_a);
            assert_eq!(final_value.len(), VALUE_LEN);
            assert_eq!(active_value.len(), VALUE_LEN);
            assert!(final_value.capacity() >= VALUE_LEN);
            assert!(active_value.capacity() >= VALUE_LEN);
            retained_payload_bytes += final_value.len() + active_value.len();
            retained_capacity_bytes +=
                final_value.capacity() + active_value.capacity();
            buffers.push(final_value.as_ptr() as usize);
            buffers.push(active_value.as_ptr() as usize);
        }
        assert_eq!(retained_payload_bytes, batch_len * 2 * VALUE_LEN);
        assert!(retained_payload_bytes <= MAX_RESULT_BYTES);
        assert!(retained_capacity_bytes <= MAX_RESULT_BYTES);
        buffers.sort_unstable();
        assert!(buffers.windows(2).all(|pair| pair[0] != pair[1]));

        println!(
            "batch_items={batch_len} rocksdb_get_read_bytes={rocksdb_get_read_bytes} rocksdb_memtable_get_count={rocksdb_memtable_get_count} retained_payload_bytes={retained_payload_bytes} retained_vec_capacity_bytes={retained_capacity_bytes} buffers={} query_elapsed_us={}",
            buffers.len(),
            elapsed.as_micros()
        );

        let (address_a_value, _) = fixture
            .controller
            .get_final_and_active_data_entry(vec![(
                fixture.address_a,
                fixture.key.clone(),
            )])
            .pop()
            .unwrap();
        let (address_b_value, _) = fixture
            .controller
            .get_final_and_active_data_entry(vec![(
                fixture.address_b,
                fixture.key.clone(),
            )])
            .pop()
            .unwrap();
        assert_eq!(address_a_value.as_deref(), Some(expected_a.as_slice()));
        assert_eq!(address_b_value.as_deref(), Some(expected_b.as_slice()));
        let missing = fixture
            .controller
            .get_final_and_active_data_entry(vec![(
                fixture.address_a,
                b"missing".to_vec(),
            )])
            .pop()
            .unwrap();
        assert_eq!(missing, (None, None));

        let original_db_value = fixture
            .final_state
            .read()
            .get_ledger()
            .get_data_entry(&fixture.address_a, &fixture.key)
            .expect("value remains in production ledger");
        let mut mutated = output;
        mutated[0].0.as_mut().unwrap()[0] ^= 0xff;
        assert_eq!(mutated[0].1.as_deref(), Some(expected_a.as_slice()));
        for (final_value, active_value) in mutated.iter().skip(1) {
            assert_eq!(final_value.as_deref(), Some(expected_a.as_slice()));
            assert_eq!(active_value.as_deref(), Some(expected_a.as_slice()));
        }
        assert_eq!(
            fixture
                .final_state
                .read()
                .get_ledger()
                .get_data_entry(&fixture.address_a, &fixture.key)
                .as_deref(),
            Some(original_db_value.as_slice())
        );
    }
}

#[test]
fn datastore_backend_active_history_preserves_final_candidate_and_missing_semantics(
) {
    let fixture = ProductionDatastoreFixture::new();
    let overwrite_key = b"overwrite".to_vec();
    let deleted_key = b"deleted".to_vec();
    let fallback_key = b"fallback".to_vec();
    insert_history_final_values(
        &fixture,
        &overwrite_key,
        &deleted_key,
        &fallback_key,
    );

    let old_value = b"candidate-old".to_vec();
    let new_value = b"candidate-new".to_vec();
    let other_address_value = b"candidate-address-b".to_vec();
    let outputs = VecDeque::from([
        update_output(
            Slot::new(1, 0),
            fixture.address_a,
            [(overwrite_key.clone(), SetOrDelete::Set(old_value))]
                .into_iter()
                .collect(),
        ),
        update_output(
            Slot::new(1, 1),
            fixture.address_b,
            [(overwrite_key.clone(), SetOrDelete::Set(other_address_value))]
                .into_iter()
                .collect(),
        ),
        update_output(
            Slot::new(1, 2),
            fixture.address_a,
            [(overwrite_key.clone(), SetOrDelete::Set(new_value.clone()))]
                .into_iter()
                .collect(),
        ),
        update_output(
            Slot::new(1, 3),
            fixture.address_a,
            [(deleted_key.clone(), SetOrDelete::Delete)]
                .into_iter()
                .collect(),
        ),
    ]);
    fixture.execution_state.read().active_history.write().0 = outputs;

    let cases = [
        (
            fixture.address_a,
            overwrite_key,
            Some(b"final-a-overwrite".to_vec()),
            Some(new_value),
        ),
        (
            fixture.address_a,
            deleted_key,
            Some(b"final-a-deleted".to_vec()),
            None,
        ),
        (
            fixture.address_a,
            fallback_key,
            Some(b"final-a-fallback".to_vec()),
            Some(b"final-a-fallback".to_vec()),
        ),
        (
            fixture.address_b,
            b"overwrite".to_vec(),
            Some(b"final-b-overwrite".to_vec()),
            Some(b"candidate-address-b".to_vec()),
        ),
        (fixture.address_a, b"absent".to_vec(), None, None),
    ];
    let input = cases
        .iter()
        .map(|(address, key, _, _)| (*address, key.clone()))
        .collect::<Vec<_>>();
    assert!(input.len() <= MAX_BATCH_ITEMS);
    let started = Instant::now();
    let output = fixture.controller.get_final_and_active_data_entry(input);
    let elapsed = started.elapsed();
    assert_eq!(output.len(), cases.len());

    let mut retained_payload_bytes = 0usize;
    let mut retained_capacity_bytes = 0usize;
    for (
        (final_value, candidate_value),
        (_, _, expected_final, expected_candidate),
    ) in output.iter().zip(cases.iter())
    {
        assert_eq!(final_value, expected_final);
        assert_eq!(candidate_value, expected_candidate);
        for value in [final_value.as_ref(), candidate_value.as_ref()]
            .into_iter()
            .flatten()
        {
            retained_payload_bytes += value.len();
            retained_capacity_bytes += value.capacity();
        }
    }
    assert!(retained_payload_bytes <= MAX_RESULT_BYTES);
    assert!(retained_capacity_bytes <= MAX_RESULT_BYTES);
    println!(
        "active_history_cases={} retained_payload_bytes={retained_payload_bytes} retained_vec_capacity_bytes={retained_capacity_bytes} query_elapsed_us={}",
        output.len(),
        elapsed.as_micros()
    );
}

fn insert_history_final_values(
    fixture: &ProductionDatastoreFixture,
    overwrite_key: &[u8],
    deleted_key: &[u8],
    fallback_key: &[u8],
) {
    let mut changes = PreHashMap::default();
    for (address, values) in [
        (
            fixture.address_a,
            vec![
                (overwrite_key, b"final-a-overwrite".as_slice()),
                (deleted_key, b"final-a-deleted".as_slice()),
                (fallback_key, b"final-a-fallback".as_slice()),
            ],
        ),
        (
            fixture.address_b,
            vec![(overwrite_key, b"final-b-overwrite".as_slice())],
        ),
    ] {
        let datastore = values
            .into_iter()
            .map(|(key, value)| (key.to_vec(), value.to_vec()))
            .collect();
        changes.insert(
            address,
            SetUpdateOrDelete::Set(LedgerEntry {
                datastore,
                ..Default::default()
            }),
        );
    }
    let mut batch = DBBatch::new();
    fixture
        .final_state
        .write()
        .get_ledger_mut()
        .apply_changes_to_batch(LedgerChanges(changes), &mut batch);
    fixture
        .db
        .write()
        .write_batch(batch, Default::default(), None);
}

fn update_output(
    slot: Slot,
    address: Address,
    datastore: BTreeMap<Vec<u8>, SetOrDelete<Vec<u8>>>,
) -> ExecutionOutput {
    let mut changes = PreHashMap::default();
    changes.insert(
        address,
        SetUpdateOrDelete::Update(LedgerEntryUpdate {
            datastore,
            ..Default::default()
        }),
    );
    ExecutionOutput {
        slot,
        block_info: None,
        state_changes: massa_final_state::StateChanges {
            ledger_changes: LedgerChanges(changes),
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

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy)]
struct ProcMemorySnapshot {
    rss_kib: u64,
    private_clean_kib: u64,
    private_dirty_kib: u64,
    private_kib: u64,
    vm_hwm_kib: u64,
}

#[cfg(target_os = "linux")]
fn read_proc_memory_snapshot() -> ProcMemorySnapshot {
    let mut smaps_rollup = String::new();
    File::open("/proc/self/smaps_rollup")
        .expect("open bounded smaps_rollup snapshot")
        .take(PROCFS_READ_LIMIT)
        .read_to_string(&mut smaps_rollup)
        .expect("read bounded smaps_rollup snapshot");
    let mut status = String::new();
    File::open("/proc/self/status")
        .expect("open bounded proc status snapshot")
        .take(PROCFS_READ_LIMIT)
        .read_to_string(&mut status)
        .expect("read bounded proc status snapshot");

    let private_clean_kib = parse_proc_kib(&smaps_rollup, "Private_Clean");
    let private_dirty_kib = parse_proc_kib(&smaps_rollup, "Private_Dirty");
    ProcMemorySnapshot {
        rss_kib: parse_proc_kib(&smaps_rollup, "Rss"),
        private_clean_kib,
        private_dirty_kib,
        private_kib: private_clean_kib + private_dirty_kib,
        vm_hwm_kib: parse_proc_kib(&status, "VmHWM"),
    }
}

#[cfg(target_os = "linux")]
fn parse_proc_kib(contents: &str, field: &str) -> u64 {
    let line = contents
        .lines()
        .find(|line| line.starts_with(&format!("{field}:")))
        .unwrap_or_else(|| panic!("missing {field} in procfs snapshot"));
    let mut columns = line.split_whitespace();
    assert_eq!(columns.next(), Some(format!("{field}: ").trim()));
    let value = columns
        .next()
        .unwrap_or_else(|| panic!("missing {field} value in procfs snapshot"))
        .parse::<u64>()
        .unwrap_or_else(|error| panic!("invalid {field} value: {error}"));
    assert_eq!(columns.next(), Some("kB"), "unexpected {field} units");
    value
}

#[cfg(target_os = "linux")]
fn log_proc_memory_snapshot(
    readers: usize,
    phase: &str,
    snapshot: ProcMemorySnapshot,
) {
    println!(
        "readers={readers} phase={phase} rss_kib={} private_clean_kib={} private_dirty_kib={} private_kib={} vm_hwm_kib={}",
        snapshot.rss_kib,
        snapshot.private_clean_kib,
        snapshot.private_dirty_kib,
        snapshot.private_kib,
        snapshot.vm_hwm_kib
    );
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct ReaderReady {
    reader: usize,
    completed_batches: usize,
    query_elapsed_us: Vec<u128>,
    retained_payload_bytes: usize,
    retained_capacity_bytes: usize,
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct ReaderFinished {
    reader: usize,
    completed_batches: usize,
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct WriterReport {
    acquisitions: usize,
    contended_attempts: usize,
    wait_elapsed_us: Vec<u128>,
}

#[cfg(target_os = "linux")]
enum WorkerReady {
    Reader(ReaderReady),
    Writer,
}

/// Characterizes retained result buffers, process memory snapshots, and write-lock admission.
/// The process metrics are whole-process counters, and the contender is not a production writer.
#[cfg(target_os = "linux")]
#[test]
fn datastore_resource_memory_and_write_lock_progress_under_bounded_reads() {
    let batch_payload_bytes = RESOURCE_BATCH_ITEMS * 2 * VALUE_LEN;
    assert!(RESOURCE_BATCH_ITEMS <= MAX_BATCH_ITEMS);
    assert!(batch_payload_bytes <= MAX_RESULT_BYTES);

    for reader_count in [1usize, 4] {
        let fixture = ProductionDatastoreFixture::new();
        let expected_value = patterned_value(0x41);
        let aggregate_payload_bytes = reader_count * batch_payload_bytes;
        assert!(aggregate_payload_bytes <= MAX_AGGREGATE_RESULT_BYTES);
        let before = read_proc_memory_snapshot();
        log_proc_memory_snapshot(reader_count, "before", before);
        let barrier = Arc::new(Barrier::new(reader_count + 2));
        let (ready_tx, ready_rx) = mpsc::sync_channel(reader_count + 1);
        let (load_started_tx, load_started_rx) =
            mpsc::sync_channel(reader_count);
        let mut release_senders = Vec::with_capacity(reader_count);

        let (reader_reports, writer_report) = thread::scope(|scope| {
            let mut readers = Vec::with_capacity(reader_count);
            for reader in 0..reader_count {
                let (release_tx, release_rx) = mpsc::sync_channel(1);
                release_senders.push(release_tx);
                let start_barrier = barrier.clone();
                let ready_tx = ready_tx.clone();
                let load_started_tx = load_started_tx.clone();
                let controller = fixture.controller.clone();
                let address = fixture.address_a;
                let key = fixture.key.clone();
                let expected_value = expected_value.clone();
                readers.push((
                    reader,
                    scope.spawn(move || {
                        start_barrier.wait();
                        let mut query_elapsed_us =
                            Vec::with_capacity(RESOURCE_BATCHES_PER_READER);
                        let mut retained_output = None;
                        let mut retained_payload_bytes = 0usize;
                        let mut retained_capacity_bytes = 0usize;

                        for batch_index in 0..RESOURCE_BATCHES_PER_READER {
                            assert!(RESOURCE_BATCH_ITEMS <= MAX_BATCH_ITEMS);
                            assert!(batch_payload_bytes <= MAX_RESULT_BYTES);
                            let input = vec![
                                (address, key.clone());
                                RESOURCE_BATCH_ITEMS
                            ];
                            if batch_index == 0 {
                                // One notification starts the contender. It may
                                // already have finished before another reader starts.
                                let _ = load_started_tx.send(());
                            }
                            let started = Instant::now();
                            let output = controller
                                .get_final_and_active_data_entry(input);
                            query_elapsed_us
                                .push(started.elapsed().as_micros());
                            assert_eq!(output.len(), RESOURCE_BATCH_ITEMS);

                            let mut payload_bytes = 0usize;
                            let mut capacity_bytes = 0usize;
                            for (final_value, active_value) in &output {
                                let final_value = final_value
                                    .as_ref()
                                    .expect("final value exists");
                                let active_value = active_value
                                    .as_ref()
                                    .expect("fallback active value exists");
                                assert_eq!(final_value, &expected_value);
                                assert_eq!(active_value, &expected_value);
                                assert_eq!(final_value.len(), VALUE_LEN);
                                assert_eq!(active_value.len(), VALUE_LEN);
                                assert!(final_value.capacity() >= VALUE_LEN);
                                assert!(active_value.capacity() >= VALUE_LEN);
                                payload_bytes +=
                                    final_value.len() + active_value.len();
                                capacity_bytes += final_value.capacity()
                                    + active_value.capacity();
                            }
                            assert_eq!(payload_bytes, batch_payload_bytes);
                            assert!(payload_bytes <= MAX_RESULT_BYTES);
                            assert!(capacity_bytes <= MAX_RESULT_BYTES);

                            if batch_index + 1 == RESOURCE_BATCHES_PER_READER {
                                retained_payload_bytes = payload_bytes;
                                retained_capacity_bytes = capacity_bytes;
                                retained_output = Some(output);
                            } else {
                                drop(output);
                            }
                        }

                        let ready = ReaderReady {
                            reader,
                            completed_batches: RESOURCE_BATCHES_PER_READER,
                            query_elapsed_us,
                            retained_payload_bytes,
                            retained_capacity_bytes,
                        };
                        ready_tx
                            .send(WorkerReady::Reader(ready))
                            .expect("send reader readiness");
                        release_rx
                            .recv_timeout(std::time::Duration::from_secs(
                                COORDINATION_TIMEOUT_SECS,
                            ))
                            .expect("reader release signal timed out");
                        drop(retained_output);
                        ReaderFinished {
                            reader,
                            completed_batches: RESOURCE_BATCHES_PER_READER,
                        }
                    }),
                ));
            }

            let start_barrier = barrier.clone();
            let writer_ready_tx = ready_tx.clone();
            let execution_state = fixture.execution_state.clone();
            let writer = scope.spawn(move || {
                start_barrier.wait();
                load_started_rx
                    .recv_timeout(std::time::Duration::from_secs(
                        COORDINATION_TIMEOUT_SECS,
                    ))
                    .expect("first reader batch did not start");
                // This is a paced lock contender; a notification is not proof that
                // a read lock is held. Only failed try_write attempts prove overlap.
                thread::yield_now();
                let mut contended_attempts = 0usize;
                let mut wait_elapsed_us =
                    Vec::with_capacity(RESOURCE_WRITE_ACQUISITIONS);
                let mut acquisitions = 0usize;
                for _ in 0..RESOURCE_WRITE_ACQUISITIONS {
                    let started = Instant::now();
                    match execution_state.try_write() {
                        Some(guard) => drop(guard),
                        None => {
                            contended_attempts += 1;
                            drop(execution_state.write());
                        }
                    }
                    acquisitions += 1;
                    wait_elapsed_us.push(started.elapsed().as_micros());
                    thread::yield_now();
                }
                writer_ready_tx
                    .send(WorkerReady::Writer)
                    .expect("send writer readiness");
                WriterReport {
                    acquisitions,
                    contended_attempts,
                    wait_elapsed_us,
                }
            });
            drop(ready_tx);
            drop(load_started_tx);
            barrier.wait();

            let mut reader_ready = Vec::with_capacity(reader_count);
            let mut writer_ready = false;
            while reader_ready.len() < reader_count || !writer_ready {
                match ready_rx
                    .recv_timeout(std::time::Duration::from_secs(
                        COORDINATION_TIMEOUT_SECS,
                    ))
                    .expect("worker readiness timed out")
                {
                    WorkerReady::Reader(report) => reader_ready.push(report),
                    WorkerReady::Writer => writer_ready = true,
                }
            }
            reader_ready.sort_by_key(|report| report.reader);
            assert_eq!(reader_ready.len(), reader_count);
            assert!(reader_ready
                .iter()
                .enumerate()
                .all(|(expected_reader, report)| report.reader
                    == expected_reader));
            assert!(reader_ready.iter().all(|report| {
                report.completed_batches == RESOURCE_BATCHES_PER_READER
                    && report.retained_payload_bytes == batch_payload_bytes
                    && report.retained_capacity_bytes <= MAX_RESULT_BYTES
                    && report.query_elapsed_us.len()
                        == RESOURCE_BATCHES_PER_READER
            }));
            let held_payload_bytes = reader_ready
                .iter()
                .map(|report| report.retained_payload_bytes)
                .sum::<usize>();
            let held_capacity_bytes = reader_ready
                .iter()
                .map(|report| report.retained_capacity_bytes)
                .sum::<usize>();
            assert!(held_payload_bytes <= MAX_AGGREGATE_RESULT_BYTES);
            assert!(held_capacity_bytes <= MAX_AGGREGATE_RESULT_BYTES);

            let held = read_proc_memory_snapshot();
            log_proc_memory_snapshot(reader_count, "held", held);
            println!(
                "readers={reader_count} procfs_held={held:?} held_payload_bytes={held_payload_bytes} held_vec_capacity_bytes={held_capacity_bytes} reader_timings_us={:?}",
                reader_ready
                    .iter()
                    .map(|report| report.query_elapsed_us.as_slice())
                    .collect::<Vec<_>>()
            );

            for release in &release_senders {
                release.send(()).expect("release held reader results");
            }
            let mut finished_readers = 0usize;
            for (expected_reader, reader) in readers {
                let finished = reader.join().expect("reader thread completed");
                assert_eq!(finished.reader, expected_reader);
                assert_eq!(
                    finished.completed_batches,
                    RESOURCE_BATCHES_PER_READER
                );
                finished_readers += 1;
            }
            assert_eq!(finished_readers, reader_count);
            let writer_report =
                writer.join().expect("writer contender completed");
            assert_eq!(writer_report.acquisitions, RESOURCE_WRITE_ACQUISITIONS);
            assert_eq!(
                writer_report.wait_elapsed_us.len(),
                RESOURCE_WRITE_ACQUISITIONS
            );
            println!(
                "readers={reader_count} writer_acquisitions={} try_write_failures={} write_waits_us={:?}",
                writer_report.acquisitions,
                writer_report.contended_attempts,
                writer_report.wait_elapsed_us
            );
            (reader_ready, writer_report)
        });

        let after = read_proc_memory_snapshot();
        log_proc_memory_snapshot(reader_count, "after", after);
        println!(
            "readers={reader_count} procfs_before={before:?} procfs_after={after:?}"
        );
        assert_eq!(reader_reports.len(), reader_count);
        assert_eq!(writer_report.acquisitions, RESOURCE_WRITE_ACQUISITIONS);
        assert!(
            writer_report.contended_attempts <= RESOURCE_WRITE_ACQUISITIONS
        );

        let recovered = fixture
            .controller
            .get_final_and_active_data_entry(vec![(
                fixture.address_a,
                fixture.key.clone(),
            )])
            .pop()
            .unwrap();
        assert_eq!(recovered.0.as_deref(), Some(expected_value.as_slice()));
        assert_eq!(recovered.1.as_deref(), Some(expected_value.as_slice()));
    }
}
