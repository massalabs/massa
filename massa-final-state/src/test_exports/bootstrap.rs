//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines tools to test the final state bootstrap

use std::sync::Arc;

use massa_async_pool::AsyncPool;
use massa_db_exports::{MassaDBController, METADATA_CF, STATE_CF, STATE_HASH_KEY};
use massa_db_worker::MassaDB;
use massa_executed_ops::{ExecutedDenunciations, ExecutedOps};
use massa_ledger_exports::LedgerController;
use massa_pos_exports::PoSFinalState;
use massa_versioning::versioning::MipStore;
use parking_lot::RwLock;

use crate::{FinalState, FinalStateConfig};

/// Create a `FinalState` from pre-set values
pub fn create_final_state(
    config: FinalStateConfig,
    ledger: Box<dyn LedgerController>,
    async_pool: AsyncPool,
    pos_state: PoSFinalState,
    executed_ops: ExecutedOps,
    executed_denunciations: ExecutedDenunciations,
    mip_store: MipStore,
    db: Arc<RwLock<Box<dyn MassaDBController>>>,
) -> FinalState {
    FinalState {
        config,
        ledger,
        async_pool,
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

    let handle_state_db1 = db1.db.cf_handle(STATE_CF).unwrap();
    let handle_state_db2 = db2.db.cf_handle(STATE_CF).unwrap();
    let iter_state_db1 = db1
        .db
        .iterator_cf(handle_state_db1, rocksdb::MassaIteratorMode::Start)
        .flatten();
    let iter_state_db2 = db2
        .db
        .iterator_cf(handle_state_db2, rocksdb::MassaIteratorMode::Start)
        .flatten();

    let handle_metadata_db1 = db1.db.cf_handle(METADATA_CF).unwrap();
    let handle_metadata_db2 = db2.db.cf_handle(METADATA_CF).unwrap();
    let iter_metadata_db1 = db1
        .db
        .iterator_cf(handle_metadata_db1, rocksdb::MassaIteratorMode::Start)
        .flatten();
    let iter_metadata_db2 = db2
        .db
        .iterator_cf(handle_metadata_db2, rocksdb::MassaIteratorMode::Start)
        .flatten();

    let count_1 = iter_state_db1.count();
    let count_2 = iter_state_db2.count();

    assert_eq!(count_1, count_2, "state count mismatch");

    let iter_state_db1 = db1
        .db
        .iterator_cf(handle_state_db1, rocksdb::MassaIteratorMode::Start)
        .flatten();
    let iter_state_db2 = db2
        .db
        .iterator_cf(handle_state_db2, rocksdb::MassaIteratorMode::Start)
        .flatten();

    let mut count = 0;
    for ((key1, value1), (key2, value2)) in iter_state_db1.zip(iter_state_db2) {
        count += 1;
        assert_eq!(key1, key2, "{}", format!("state key mismatch {}", count));
        assert_eq!(
            value1,
            value2,
            "{}",
            format!("state value n°{} mismatch for key {:?} ", count, key1)
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
        v1.db.read().get_db_hash(),
        v2.db.read().get_db_hash(),
        "rocks_db hash mismatch"
    );
}
