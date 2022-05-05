use std::fmt::format;

use massa_models::Address;
use rocksdb::{Error, WriteBatch, DB};

use crate::{ledger_changes::LedgerEntryUpdate, LedgerEntry, SetOrKeep};

const DB_PATH: &str = "_path_to_db";
const OPEN_ERROR: &str = "critical: rocksdb open operation failed";
const CRUD_ERROR: &str = "critical: rocksdb crud operation failed";

pub(crate) struct LedgerDB(DB);

macro_rules! balance_key {
    ($addr:ident) => {
        format!("{}:balance", $addr).as_bytes()
    };
}

macro_rules! bytecode_key {
    ($addr:ident) => {
        format!("{}:bytecode", $addr).as_bytes()
    };
}

macro_rules! datastore_key {
    ($addr:ident, $hash:ident) => {
        format!("{}:datastore:{}", $addr, $hash).as_bytes()
    };
}

impl LedgerDB {
    fn new() -> Self {
        LedgerDB(DB::open_default(DB_PATH).expect(OPEN_ERROR))
    }

    fn put(&self, addr: Address, ledger_entry: LedgerEntry) {
        let mut batch = WriteBatch::default();
        batch.put(
            balance_key!(addr),
            ledger_entry.parallel_balance.to_raw().to_be_bytes(),
        );
        batch.put(bytecode_key!(addr), ledger_entry.bytecode);
        for (hash, entry) in ledger_entry.datastore {
            batch.put(datastore_key!(addr, hash), entry);
        }
        self.0.write(batch).expect(CRUD_ERROR);
    }

    fn update(&self, addr: Address, entry_update: LedgerEntryUpdate) {
        let mut batch = WriteBatch::default();
        if let SetOrKeep::Set(balance) = entry_update.parallel_balance {
            batch.put(balance_key!(addr), balance.to_raw().to_be_bytes());
        }
        if let SetOrKeep::Set(bytecode) = entry_update.bytecode {
            batch.put(bytecode_key!(addr), bytecode);
        }
        for (hash, entry) in entry_update.datastore {}
        self.0.write(batch).expect(CRUD_ERROR);
    }
}
