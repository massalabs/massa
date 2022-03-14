// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::collections::BTreeMap;

use massa_models::{Address, Slot};

use crate::{FinalLedgerBootstrapState, LedgerEntry};

/// This file defines tools to test the ledger bootstrap

/// creates a ledger bootstrap state from components
pub fn make_bootstrap_state(
    slot: Slot,
    sorted_ledger: BTreeMap<Address, LedgerEntry>,
) -> FinalLedgerBootstrapState {
    FinalLedgerBootstrapState {
        slot,
        sorted_ledger,
    }
}

/// asserts that two ledger entries are the same
pub fn assert_eq_ledger_entry(v1: &LedgerEntry, v2: &LedgerEntry) {
    assert_eq!(
        v1.parallel_balance, v2.parallel_balance,
        "parallel balance mismatch"
    );
    assert_eq!(v1.bytecode, v2.bytecode, "bytecode mismatch");
    assert_eq!(
        v1.datastore.len(),
        v2.datastore.len(),
        "datastore len mismatch"
    );
    for k in v1.datastore.keys() {
        let itm1 = v1.datastore.get(k).unwrap();
        let itm2 = v2.datastore.get(k).expect("datastore key mismatch");
        assert_eq!(itm1, itm2, "datasore entry mismatch");
    }
}

/// asserts that two FinalLedgerBootstrapState are equal
pub fn assert_eq_ledger_bootstrap_state(
    v1: &FinalLedgerBootstrapState,
    v2: &FinalLedgerBootstrapState,
) {
    assert_eq!(v1.slot, v2.slot, "final slot mismatch");
    assert_eq!(
        v1.sorted_ledger.len(),
        v2.sorted_ledger.len(),
        "ledger len mismatch"
    );
    for k in v1.sorted_ledger.keys() {
        let itm1 = v1.sorted_ledger.get(k).unwrap();
        let itm2 = v2.sorted_ledger.get(k).expect("ledger key mismatch");
        assert_eq_ledger_entry(itm1, itm2);
    }
}
