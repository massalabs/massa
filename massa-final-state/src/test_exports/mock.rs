//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines tools to test the final state bootstrap
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::Seek,
    str::FromStr,
    sync::Arc,
};

use crate::{controller_trait::FinalStateController, FinalState, FinalStateConfig};
use massa_async_pool::AsyncPool;
use massa_db_exports::{
    DBBatch, MassaIteratorMode, ShareableMassaDBController, METADATA_CF, STATE_CF, STATE_HASH_KEY,
};
use massa_deferred_calls::DeferredCallRegistry;
use massa_executed_ops::{ExecutedDenunciations, ExecutedOps};
use massa_ledger_exports::{LedgerConfig, LedgerController, LedgerEntry, LedgerError};
use massa_ledger_worker::FinalLedger;
use massa_models::{
    address::Address,
    amount::Amount,
    config::{ENDORSEMENT_COUNT, GENESIS_TIMESTAMP, T0, THREAD_COUNT},
};
use massa_pos_exports::{PoSFinalState, SelectorController};
use massa_signature::KeyPair;
use massa_versioning::versioning::MipStore;
use parking_lot::RwLock;
use tempfile::NamedTempFile;

#[allow(clippy::too_many_arguments)]
/// Create a `FinalState` from pre-set values
pub fn create_final_state(
    config: FinalStateConfig,
    ledger: Box<dyn LedgerController>,
    async_pool: AsyncPool,
    deferred_call_registry: DeferredCallRegistry,
    pos_state: PoSFinalState,
    executed_ops: ExecutedOps,
    executed_denunciations: ExecutedDenunciations,
    mip_store: MipStore,
    db: ShareableMassaDBController,
) -> FinalState {
    FinalState {
        config,
        ledger,
        async_pool,
        deferred_call_registry,
        pos_state,
        executed_ops,
        executed_denunciations,
        mip_store,
        last_start_period: 0,
        last_slot_before_downtime: None,
        db,
    }
}

/// asserts that two `FinalState` are equal
pub fn assert_eq_final_state(v1: &FinalState, v2: &FinalState) {
    assert_eq!(
        v1.db.read().get_change_id().unwrap(),
        v2.db.read().get_change_id().unwrap(),
        "final slot mismatch"
    );
    assert_eq!(
        v1.last_start_period, v2.last_start_period,
        "last_start_period mismatch"
    );
    assert_eq!(
        v1.last_slot_before_downtime, v2.last_slot_before_downtime,
        "last_slot_before_downtime mismatch"
    );

    let db1 = v1.db.read();
    let db2 = v2.db.read();

    let iter_state_db1 = db1.iterator_cf(STATE_CF, MassaIteratorMode::Start);
    let iter_state_db2 = db2.iterator_cf(STATE_CF, MassaIteratorMode::Start);

    let iter_metadata_db1 = db1.iterator_cf(METADATA_CF, MassaIteratorMode::Start);
    let iter_metadata_db2 = db2.iterator_cf(METADATA_CF, MassaIteratorMode::Start);

    let count_1 = iter_state_db1.count();
    let count_2 = iter_state_db2.count();

    assert_eq!(count_1, count_2, "state count mismatch");

    let iter_state_db1 = db1.iterator_cf(STATE_CF, MassaIteratorMode::Start);
    let iter_state_db2 = db2.iterator_cf(STATE_CF, MassaIteratorMode::Start);

    let mut count = 0;
    for ((key1, value1), (key2, value2)) in iter_state_db1.zip(iter_state_db2) {
        count += 1;
        assert_eq!(key1, key2, "state key mismatch {}", count);
        assert_eq!(
            value1, value2,
            "state value n°{} mismatch for key {:?} ",
            count, key1
        );
    }

    for ((key1, value1), (key2, value2)) in iter_metadata_db1.zip(iter_metadata_db2) {
        assert_eq!(key1, key2, "metadata key mismatch");
        if key1.to_vec() != STATE_HASH_KEY.to_vec() {
            assert_eq!(value1, value2, "metadata value mismatch");
        }
    }

    assert_eq!(
        v1.pos_state.cycle_history_cache, v2.pos_state.cycle_history_cache,
        "pos_state.cycle_history_cache mismatch"
    );
    assert_eq!(
        v1.pos_state.rng_seed_cache, v2.pos_state.rng_seed_cache,
        "pos_state.rng_seed_cache mismatch"
    );

    assert_eq!(
        v1.async_pool.message_info_cache.len(),
        v2.async_pool.message_info_cache.len(),
        "async_pool.message_info_cache len mismatch"
    );

    assert_eq!(
        v1.async_pool.message_info_cache, v2.async_pool.message_info_cache,
        "async_pool.message_info_cache mismatch"
    );
}

/// asserts that two `FinalState` hashes are equal
pub fn assert_eq_final_state_hash(v1: &FinalState, v2: &FinalState) {
    assert_eq!(
        v1.db.read().get_xof_db_hash(),
        v2.db.read().get_xof_db_hash(),
        "rocks_db hash mismatch"
    );
}

/// New initials
pub fn get_initials() -> (NamedTempFile, HashMap<Address, LedgerEntry>) {
    let file: NamedTempFile = NamedTempFile::new().unwrap();
    let mut rolls: BTreeMap<Address, u64> = BTreeMap::new();
    let mut ledger: HashMap<Address, LedgerEntry> = HashMap::new();

    let raw_keypairs = [
        "S18r2i8oJJyhF7Kprx98zwxAc3W4szf7RKuVMX6JydZz8zSxHeC", // thread 0
        "S1FpYC4ugG9ivZZbLVrTwWtF9diSRiAwwrVX5Gx1ANSRLfouUjq", // thread 1
        "S1LgXhWLEgAgCX3nm6y8PVPzpybmsYpi6yg6ZySwu5Z4ERnD7Bu", // thread 2
    ];

    for s in raw_keypairs {
        let keypair = KeyPair::from_str(s).unwrap();
        let addr = Address::from_public_key(&keypair.get_public_key());
        rolls.insert(addr, 100);
        ledger.insert(
            addr,
            LedgerEntry {
                balance: Amount::from_str("300_000").unwrap(),
                ..Default::default()
            },
        );
    }

    // write file
    serde_json::to_writer_pretty::<&File, BTreeMap<Address, u64>>(file.as_file(), &rolls)
        .expect("unable to write ledger file");
    file.as_file()
        .seek(std::io::SeekFrom::Start(0))
        .expect("could not seek file");

    (file, ledger)
}

/// Get a final state
#[allow(clippy::type_complexity)]
pub fn get_sample_state(
    last_start_period: u64,
    selector_controller: Box<dyn SelectorController>,
    mip_store: MipStore,
    db: ShareableMassaDBController,
) -> Result<(Arc<RwLock<dyn FinalStateController>>, NamedTempFile), LedgerError> {
    let (rolls_file, ledger) = get_initials();
    let (ledger_config, tempfile) = LedgerConfig::sample(&ledger);

    let mut ledger = FinalLedger::new(ledger_config.clone(), db.clone());
    ledger.load_initial_ledger().unwrap();
    let default_config = FinalStateConfig::default();
    let cfg = FinalStateConfig {
        ledger_config,
        async_pool_config: default_config.async_pool_config,
        pos_config: default_config.pos_config,
        executed_ops_config: default_config.executed_ops_config,
        executed_denunciations_config: default_config.executed_denunciations_config,
        final_history_length: 128,
        thread_count: THREAD_COUNT,
        initial_rolls_path: rolls_file.path().to_path_buf(),
        endorsement_count: ENDORSEMENT_COUNT,
        max_executed_denunciations_length: 1000,
        initial_seed_string: "".to_string(),
        periods_per_cycle: 10,
        max_denunciations_per_block_header: 0,
        t0: T0,
        genesis_timestamp: *GENESIS_TIMESTAMP,
        ledger_backup_periods_interval: 10,
    };

    let mut final_state = if last_start_period > 0 {
        FinalState::new_derived_from_snapshot(
            db.clone(),
            cfg,
            Box::new(ledger),
            selector_controller,
            mip_store,
            last_start_period,
        )
        .unwrap()
    } else {
        FinalState::new(
            db.clone(),
            cfg,
            Box::new(ledger),
            selector_controller,
            mip_store,
            true,
        )
        .unwrap()
    };

    let mut batch: BTreeMap<Vec<u8>, Option<Vec<u8>>> = DBBatch::new();
    final_state.pos_state.create_initial_cycle(&mut batch);
    final_state.init_execution_trail_hash_to_batch(&mut batch);
    final_state
        .db
        .write()
        .write_batch(batch, Default::default(), None);
    final_state.compute_initial_draws().unwrap();
    Ok((Arc::new(RwLock::new(final_state)), tempfile))
}
