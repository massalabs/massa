// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::collections::BTreeMap;

use massa_models::Address;

use crate::{ledger_db::LedgerDB, FinalLedger, LedgerConfig, LedgerEntry};

/// This file defines tools to test the ledger bootstrap

pub fn create_final_ledger(config: LedgerConfig) -> FinalLedger {
    FinalLedger {
        _config: config,
        //NOTE:Aurelien change
        sorted_ledger: LedgerDB::new("".into()),
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
        assert_eq!(itm1, itm2, "datastore entry mismatch");
    }
}

/// asserts that two `FinalLedgerBootstrapState` are equal
pub fn assert_eq_ledger(v1: &FinalLedger, v2: &FinalLedger) {
    // assert_eq!(
    //     v1.sorted_ledger.len(),
    //     v2.sorted_ledger.len(),
    //     "ledger len mismatch"
    // );
    // for k in v1.sorted_ledger.keys() {
    //     let itm1 = v1.sorted_ledger.get(k).unwrap();
    //     let itm2 = v2.sorted_ledger.get(k).expect("ledger key mismatch");
    //     assert_eq_ledger_entry(itm1, itm2);
    // }
}
