//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines tools to test the final state bootstrap

use crate::{FinalState, FinalStateBootstrap};
use massa_async_pool::AsyncMessage;
use massa_ledger::LedgerEntry;
use massa_models::{Address, Slot};
use std::collections::BTreeMap;

/// creates a final state bootstrap from components
pub fn make_bootstrap_state(
    slot: Slot,
    sorted_ledger: BTreeMap<Address, LedgerEntry>,
    messages: Vec<AsyncMessage>,
) -> FinalStateBootstrap {
    FinalStateBootstrap {
        slot,
        ledger: massa_ledger::test_exports::make_bootstrap_state(sorted_ledger),
        async_pool: massa_async_pool::test_exports::make_bootstrap_state(messages),
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
