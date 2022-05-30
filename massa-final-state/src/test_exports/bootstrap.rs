//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines tools to test the final state bootstrap

use std::collections::VecDeque;

use massa_async_pool::AsyncPool;
use massa_ledger::FinalLedger;
use massa_models::Slot;

use crate::{FinalState, FinalStateConfig, StateChanges};

/// Create a `FinalState` from pre-set values
pub fn create_final_state(
    config: FinalStateConfig,
    slot: Slot,
    ledger: FinalLedger,
    async_pool: AsyncPool,
    changes_history: VecDeque<(Slot, StateChanges)>,
) -> FinalState {
    FinalState {
        config,
        slot,
        ledger,
        async_pool,
        changes_history,
    }
}

/// asserts that two `FinalState` are equal
pub fn assert_eq_final_state(v1: &FinalState, v2: &FinalState) {
    // compare slots
    assert_eq!(v1.slot, v2.slot, "final slot mismatch");

    // compare ledger states
    massa_ledger::test_exports::assert_eq_ledger(&v1.ledger, &v2.ledger);
    massa_async_pool::test_exports::assert_eq_async_pool_bootstrap_state(
        &v1.async_pool,
        &v2.async_pool,
    );
}
