use massa_hash::Hash;
use massa_models::{Address, Amount};
use rocksdb::{WriteBatch, DB};
use std::collections::BTreeMap;

use crate::{ledger_changes::LedgerEntryUpdate, LedgerEntry, SetOrDelete, SetOrKeep};

const DB_PATH: &str = "_path_to_db";
const OPEN_ERROR: &str = "critical: rocksdb open failed";
const CRUD_ERROR: &str = "critical: rocksdb crud operation failed";

pub(crate) enum LedgerDBEntry {
    Balance,
    Bytecode,
    Datastore(Hash),
}

pub(crate) struct LedgerDB(DB);

// IMPORTANT NOTES:
// - use cf instead of key formatting
// - find a way to open datastore cf's on new db
// - might not need to have a mutex on ledger with multi threaded disk db

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
    pub fn new() -> Self {
        LedgerDB(DB::open_default(DB_PATH).expect(OPEN_ERROR))
    }

    pub fn put(&self, addr: &Address, ledger_entry: LedgerEntry) {
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

    pub fn update(&self, addr: &Address, entry_update: LedgerEntryUpdate) {
        let mut batch = WriteBatch::default();
        if let SetOrKeep::Set(balance) = entry_update.parallel_balance {
            batch.put(balance_key!(addr), balance.to_raw().to_be_bytes());
        }
        if let SetOrKeep::Set(bytecode) = entry_update.bytecode {
            batch.put(bytecode_key!(addr), bytecode);
        }
        for (hash, update) in entry_update.datastore {
            match update {
                SetOrDelete::Set(entry) => batch.put(datastore_key!(addr, hash), entry),
                SetOrDelete::Delete => {
                    if self.0.key_may_exist(datastore_key!(addr, hash)) {
                        batch.delete(datastore_key!(addr, hash));
                    }
                }
            }
        }
        self.0.write(batch).expect(CRUD_ERROR);
    }

    pub fn delete(&self, _addr: &Address) {
        // note: missing delete
    }

    pub fn entry_exists(&self, addr: &Address, ty: LedgerDBEntry) -> bool {
        match ty {
            LedgerDBEntry::Balance => self.0.key_may_exist(balance_key!(addr)),
            LedgerDBEntry::Bytecode => self.0.key_may_exist(bytecode_key!(addr)),
            LedgerDBEntry::Datastore(hash) => self.0.key_may_exist(datastore_key!(addr, hash)),
        }
    }

    pub fn get_entry(&self, addr: &Address, ty: LedgerDBEntry) -> Option<Vec<u8>> {
        match ty {
            LedgerDBEntry::Balance => self.0.get(balance_key!(addr)).expect(CRUD_ERROR),
            LedgerDBEntry::Bytecode => self.0.get(bytecode_key!(addr)).expect(CRUD_ERROR),
            LedgerDBEntry::Datastore(hash) => {
                self.0.get(datastore_key!(addr, hash)).expect(CRUD_ERROR)
            }
        }
    }

    pub fn get_full_entry(&self, addr: &Address) -> Option<LedgerEntry> {
        // note: think twice about this conversion
        if let Some(parallel_balance) = self.get_entry(addr, LedgerDBEntry::Balance).map(|bytes| {
            Amount::from_raw(u64::from_be_bytes(
                bytes.try_into().expect("critical: invalid balance format"),
            ))
        }) {
            Some(LedgerEntry {
                parallel_balance,
                bytecode: self
                    .get_entry(addr, LedgerDBEntry::Bytecode)
                    .unwrap_or_else(|| Vec::new()),
                // note: missing datastore
                datastore: BTreeMap::new(),
            })
        } else {
            None
        }
    }
}
