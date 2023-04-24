//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines tools to test the final state bootstrap

use std::{collections::VecDeque, sync::Arc};

use massa_async_pool::AsyncPool;
use massa_executed_ops::{ExecutedDenunciations, ExecutedOps};
use massa_hash::{Hash, HASH_SIZE_BYTES};
use massa_ledger_exports::LedgerController;
use massa_models::slot::Slot;
use massa_pos_exports::PoSFinalState;
use parking_lot::RwLock;
use rocksdb::DB;

use crate::{FinalState, FinalStateConfig, StateChanges};

/// Create a `FinalState` from pre-set values
pub fn create_final_state(
    config: FinalStateConfig,
    slot: Slot,
    ledger: Box<dyn LedgerController>,
    async_pool: AsyncPool,
    changes_history: VecDeque<(Slot, StateChanges)>,
    pos_state: PoSFinalState,
    executed_ops: ExecutedOps,
    processed_denunciations: ExecutedDenunciations,
    rocks_db: Arc<RwLock<DB>>,
) -> FinalState {
    FinalState {
        config,
        slot,
        ledger,
        async_pool,
        changes_history,
        pos_state,
        executed_ops,
        executed_denunciations: processed_denunciations,
        final_state_hash: Hash::from_bytes(&[0; HASH_SIZE_BYTES]),
        last_start_period: 0,
        rocks_db,
    }
}

/// asserts that two `FinalState` are equal
pub fn assert_eq_final_state(v1: &FinalState, v2: &FinalState) {
    // compare slot
    assert_eq!(v1.slot, v2.slot, "final slot mismatch");

    // compare final state
    massa_ledger_worker::test_exports::assert_eq_ledger(&*v1.ledger, &*v2.ledger);
    massa_async_pool::test_exports::assert_eq_async_pool_bootstrap_state(
        &v1.async_pool,
        &v2.async_pool,
    );
    massa_pos_exports::test_exports::assert_eq_pos_state(&v1.pos_state, &v2.pos_state);
    assert_eq!(
        v1.executed_ops.ops.len(),
        v2.executed_ops.ops.len(),
        "executed_ops.ops lenght mismatch"
    );
    assert_eq!(
        v1.executed_ops.ops, v2.executed_ops.ops,
        "executed_ops.ops mismatch"
    );
    assert_eq!(
        v1.executed_ops.sorted_ops, v2.executed_ops.sorted_ops,
        "executed_ops.sorted_ops mismatch"
    );
}

/// asserts that two `FinalState` hashes are equal
pub fn assert_eq_final_state_hash(v1: &FinalState, v2: &FinalState) {
    assert_eq!(
        v1.ledger.get_ledger_hash(),
        v2.ledger.get_ledger_hash(),
        "ledger hash mismatch"
    );
    assert_eq!(
        v1.async_pool.get_hash(),
        v2.async_pool.get_hash(),
        "async pool hash mismatch"
    );
    assert_eq!(
        v1.pos_state.deferred_credits.get_hash(),
        v2.pos_state.deferred_credits.get_hash(),
        "deferred credits hash mismatch"
    );
    for (cycle1, cycle2) in v1
        .pos_state
        .cycle_history
        .iter()
        .zip(v2.pos_state.cycle_history.iter())
    {
        assert_eq!(
            cycle1.roll_counts_hash, cycle2.roll_counts_hash,
            "cycle ({}) roll_counts_hash mismatch",
            cycle1.cycle
        );
        assert_eq!(
            cycle1.production_stats_hash, cycle2.production_stats_hash,
            "cycle ({}) roll_counts_hash mismatch",
            cycle1.cycle
        );
        assert_eq!(
            cycle1.cycle_global_hash, cycle2.cycle_global_hash,
            "cycle ({}) global_hash mismatch",
            cycle1.cycle
        );
    }
    assert_eq!(
        v1.executed_ops.hash, v2.executed_ops.hash,
        "executed ops hash mismatch"
    );
}
