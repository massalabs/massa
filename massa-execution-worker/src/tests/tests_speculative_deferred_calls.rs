// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Unit tests for `SpeculativeDeferredCallRegistry`.
//!
//! These tests focus on the layered lookup semantics across
//! `deferred_calls_changes` (current speculative layer), `active_history`
//! (recently executed but not yet finalized slots) and the final state
//! registry. In particular they cover the regression around `Delete`
//! tombstones being lossy in `get_call`, which previously allowed a
//! consumed deferred call to be resurrected from a stale layer and
//! double-refunded by `cancel_call` (coin inflation).

use std::collections::VecDeque;
use std::str::FromStr;
use std::sync::Arc;

use parking_lot::RwLock;

use massa_deferred_calls::{
    config::DeferredCallsConfig, registry_changes::DeferredCallRegistryChanges, DeferredCall,
};
use massa_execution_exports::ExecutionOutput;
use massa_final_state::{MockFinalStateController, StateChanges};
use massa_models::{address::Address, amount::Amount, deferred_calls::DeferredCallId, slot::Slot};

use crate::active_history::ActiveHistory;
use crate::speculative_deferred_calls::SpeculativeDeferredCallRegistry;

/// Build a minimal `ExecutionOutput` carrying only the provided deferred-call
/// changes. Every other field is left at its default so the test stays
/// focused on the deferred-call lookup behaviour.
fn make_history_item(changes: DeferredCallRegistryChanges) -> ExecutionOutput {
    ExecutionOutput {
        slot: Slot::new(1, 0),
        block_info: None,
        state_changes: StateChanges {
            ledger_changes: Default::default(),
            async_pool_changes: Default::default(),
            deferred_call_changes: changes,
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

fn make_call(target_slot: Slot) -> DeferredCall {
    DeferredCall::new(
        Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
        target_slot,
        Address::from_str("AS127QtY6Hzm6BnJc9wqCBfPNvEH9fKer3LiMNNQmcX3MzLwCL6G6").unwrap(),
        "receive".to_string(),
        vec![1, 2, 3, 4],
        Amount::from_raw(100),
        3_000_000,
        Amount::from_raw(1),
        false,
    )
}

/// Regression test for the deferred-call tombstone resurrection bug.
///
/// Setup:
/// * `active_history` contains a `Set(call)` for `(slot, id)` (the call
///   was registered in an earlier slot and finalized into history).
/// * The speculative layer holds an explicit `Delete` for that same id
///   (simulating `advance_slot` having deleted the call after executing
///   it for the current slot).
///
/// Before the fix, `get_call` treated the speculative `Delete` as
/// "not found here" and fell through to the active_history layer,
/// returning the stale call. `cancel_call` then resurrected the removed
/// entry and triggered a refund through `transfer_coins(None, sender,
/// coins, ...)`, minting coins out of thin air.
///
/// After the fix, a `Delete` tombstone is terminal: `get_call` returns
/// `None` and `cancel_call` fails with `"Call ID does not exist."`.
#[test]
fn get_call_speculative_delete_blocks_history() {
    let target_slot = Slot {
        thread: 5,
        period: 1,
    };
    let id = DeferredCallId::new(0, target_slot, 0, &[]).unwrap();
    let call = make_call(target_slot);

    // Layer 1: active_history holds a Set(call) for this id.
    let mut history_changes = DeferredCallRegistryChanges::default();
    history_changes.set_call(id.clone(), call);
    let active_history = Arc::new(RwLock::new(ActiveHistory(VecDeque::from([
        make_history_item(history_changes),
    ]))));

    // final_state mock: must not be touched. A `Delete` tombstone in the
    // newest layer should short-circuit the lookup before the final state
    // is consulted. We deliberately set no expectations so any access
    // would panic and surface a regression.
    let mock_final_state = Arc::new(RwLock::new(MockFinalStateController::new()));

    let mut speculative = SpeculativeDeferredCallRegistry::new(
        mock_final_state,
        active_history,
        DeferredCallsConfig::default(),
    );

    // Sanity: without a tombstone, the call is visible from history.
    assert!(
        speculative.get_call(&id).is_some(),
        "sanity check: history Set should be visible before the Delete is placed"
    );

    // Layer 2: place a Delete tombstone in the speculative layer.
    speculative.delete_call(&id, target_slot);

    assert!(
        speculative.get_call(&id).is_none(),
        "Delete tombstone must terminate the lookup cascade and prevent \
         resurrection of the stale call"
    );

    let err = speculative
        .cancel_call(&id)
        .expect_err("cancel_call must fail for a deleted call");
    let msg = format!("{}", err);
    assert!(
        msg.contains("Call ID does not exist"),
        "unexpected error message: {msg}"
    );
}

/// Same property as above but with the tombstone living inside the
/// `active_history` (newer history item has `Delete`, older history item
/// has `Set`). This guards against a future regression that would only
/// fix the speculative layer and forget the history walk.
#[test]
fn get_call_history_delete_blocks_older_history_set() {
    let target_slot = Slot {
        thread: 5,
        period: 1,
    };
    let id = DeferredCallId::new(0, target_slot, 0, &[]).unwrap();
    let call = make_call(target_slot);

    // Oldest history item: Set(call).
    let mut older_changes = DeferredCallRegistryChanges::default();
    older_changes.set_call(id.clone(), call);

    // Newer history item: Delete.
    let mut newer_changes = DeferredCallRegistryChanges::default();
    newer_changes.delete_call(target_slot, &id);

    // `ActiveHistory` is oldest-first; the speculative loop iterates
    // `.rev()` so the newer (Delete) item is visited first.
    let active_history = Arc::new(RwLock::new(ActiveHistory(VecDeque::from([
        make_history_item(older_changes),
        make_history_item(newer_changes),
    ]))));

    let mock_final_state = Arc::new(RwLock::new(MockFinalStateController::new()));

    let speculative = SpeculativeDeferredCallRegistry::new(
        mock_final_state,
        active_history,
        DeferredCallsConfig::default(),
    );

    assert!(
        speculative.get_call(&id).is_none(),
        "Delete in newer history must terminate the cascade and hide the \
         older Set"
    );
}
