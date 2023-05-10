// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_ledger_exports::{LedgerConfig, LedgerController, LedgerEntry};
use massa_models::address::Address;
use std::collections::HashMap;
use tempfile::TempDir;

use crate::{ledger_db::LedgerDB, FinalLedger};

/// This file defines tools to test the ledger bootstrap

pub fn create_final_ledger(
    config: LedgerConfig,
    initial_ledger: HashMap<Address, LedgerEntry>,
) -> FinalLedger {
    let temp_dir = TempDir::new().unwrap();
    let mut db = LedgerDB::new(
        temp_dir.path().to_path_buf(),
        config.thread_count,
        config.max_key_length,
        config.max_ledger_part_size,
        false,
    );
    db.load_initial_ledger(initial_ledger);
    FinalLedger {
        config,
        sorted_ledger: db,
    }
}

/// asserts that two ledger entries are the same
pub(crate) fn assert_eq_ledger_entry(v1: &LedgerEntry, v2: &LedgerEntry) {
    assert_eq!(v1.balance, v2.balance, "balance mismatch");
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
pub fn assert_eq_ledger(v1: &dyn LedgerController, v2: &dyn LedgerController) {
    let ledger1: HashMap<Address, LedgerEntry> = v1
        .get_every_address()
        .iter()
        .map(|(addr, balance)| {
            (
                *addr,
                LedgerEntry {
                    balance: *balance,
                    bytecode: v1.get_bytecode(addr).unwrap_or_default(),
                    datastore: v1.get_entire_datastore(addr),
                },
            )
        })
        .collect();
    let ledger2: HashMap<Address, LedgerEntry> = v2
        .get_every_address()
        .iter()
        .map(|(addr, balance)| {
            (
                *addr,
                LedgerEntry {
                    balance: *balance,
                    bytecode: v2.get_bytecode(addr).unwrap_or_default(),
                    datastore: v2.get_entire_datastore(addr),
                },
            )
        })
        .collect();
    assert_eq!(ledger1.len(), ledger2.len(), "ledger len mismatch");
    for k in ledger1.keys() {
        let itm1 = ledger1.get(k).unwrap();
        let itm2 = ledger2.get(k).expect("ledger key mismatch");
        assert_eq_ledger_entry(itm1, itm2);
    }
}
