// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_hash::Hash;
use massa_models::{Address, Amount};
use rocksdb::{Options, WriteBatch, DB};
use std::collections::BTreeMap;

use crate::{ledger_changes::LedgerEntryUpdate, LedgerEntry, SetOrDelete, SetOrKeep};

const DB_PATH: &str = "../ledger_db";
const BALANCE_CF: &str = "balance";
const BYTECODE_CF: &str = "bytecode";
const OPEN_ERROR: &str = "critical: rocksdb open operation failed";
const CRUD_ERROR: &str = "critical: rocksdb crud operation failed";
const CF_ERROR: &str = "critical: rocksdb column family operation failed";

pub(crate) enum LedgerDBEntry {
    Balance,
    Bytecode,
    Datastore(Hash),
}

pub(crate) struct LedgerDB(DB);

impl LedgerDB {
    pub fn new() -> Self {
        // options
        let mut opts = Options::default();
        opts.create_if_missing(true);

        // database init
        let mut db = DB::open(&opts, DB_PATH).expect(OPEN_ERROR);
        db.create_cf(BALANCE_CF, &Options::default())
            .expect(CF_ERROR);
        db.create_cf(BYTECODE_CF, &Options::default())
            .expect(CF_ERROR);

        // return database
        LedgerDB(db)
    }

    pub fn put(&mut self, addr: &Address, ledger_entry: LedgerEntry) {
        let mut batch = WriteBatch::default();
        let key = addr.to_bytes();

        // balance
        batch.put_cf(
            self.0.cf_handle(BALANCE_CF).expect(CF_ERROR),
            key,
            ledger_entry.parallel_balance.to_raw().to_be_bytes(),
        );

        // bytecode
        batch.put_cf(
            self.0.cf_handle(BYTECODE_CF).expect(CF_ERROR),
            key,
            ledger_entry.bytecode,
        );

        // datastore
        if !ledger_entry.datastore.is_empty() {
            // note: this might be enough
            let cf_name = addr.to_string();
            self.0
                .create_cf(&cf_name, &Options::default())
                .expect(CF_ERROR);
            let cf = self.0.cf_handle(&cf_name).expect(CF_ERROR);
            for (hash, entry) in ledger_entry.datastore {
                let data_key = hash.to_bytes();
                batch.put_cf(&cf, data_key, entry);
            }
        }

        // write batch
        self.0.write(batch).expect(CRUD_ERROR);
    }

    pub fn update(&mut self, addr: &Address, entry_update: LedgerEntryUpdate) {
        let mut batch = WriteBatch::default();
        let key = addr.to_bytes();

        // balance
        if let SetOrKeep::Set(balance) = entry_update.parallel_balance {
            batch.put_cf(
                self.0.cf_handle(BALANCE_CF).expect(CF_ERROR),
                key,
                balance.to_raw().to_be_bytes(),
            );
        }

        // bytecode
        if let SetOrKeep::Set(bytecode) = entry_update.bytecode {
            batch.put_cf(
                self.0.cf_handle(BYTECODE_CF).expect(CF_ERROR),
                key,
                bytecode,
            );
        }

        // datastore
        if !entry_update.datastore.is_empty() {
            let cf_name = addr.to_string();
            let cf = match self.0.cf_handle(&cf_name) {
                Some(cf) => cf,
                None => {
                    self.0
                        .create_cf(&cf_name, &Options::default())
                        .expect(CF_ERROR);
                    self.0.cf_handle(&cf_name).expect(CF_ERROR)
                }
            };
            for (hash, update) in entry_update.datastore {
                let data_key = hash.to_bytes();
                match update {
                    SetOrDelete::Set(entry) => batch.put_cf(&cf, data_key, entry),
                    SetOrDelete::Delete => batch.delete_cf(&cf, data_key),
                }
            }
        }

        // write batch
        self.0.write(batch).expect(CRUD_ERROR);
    }

    pub fn delete(&self, _addr: &Address) {
        // note: missing delete
    }

    pub fn entry_exists(&self, addr: &Address, ty: LedgerDBEntry) -> bool {
        let key = addr.to_bytes();
        match ty {
            LedgerDBEntry::Balance => self
                .0
                .cf_handle(BALANCE_CF)
                .is_some_and(|cf| self.0.key_may_exist_cf(cf, key)),
            LedgerDBEntry::Bytecode => self
                .0
                .cf_handle(BYTECODE_CF)
                .is_some_and(|cf| self.0.key_may_exist_cf(cf, key)),
            LedgerDBEntry::Datastore(hash) => self
                .0
                .cf_handle(&addr.to_string())
                .is_some_and(|cf| self.0.key_may_exist_cf(cf, hash.to_bytes())),
        }
    }

    pub fn get_entry(&self, addr: &Address, ty: LedgerDBEntry) -> Option<Vec<u8>> {
        let key = addr.to_bytes();
        match ty {
            LedgerDBEntry::Balance => self
                .0
                .cf_handle(BALANCE_CF)
                .map(|cf| self.0.get_cf(cf, key).expect(CRUD_ERROR))
                .flatten(),
            LedgerDBEntry::Bytecode => self
                .0
                .cf_handle(BYTECODE_CF)
                .map(|cf| self.0.get_cf(cf, key).expect(CRUD_ERROR))
                .flatten(),
            LedgerDBEntry::Datastore(hash) => self
                .0
                .cf_handle(&addr.to_string())
                .map(|cf| self.0.get_cf(cf, hash.to_bytes()).expect(CRUD_ERROR))
                .flatten(),
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

#[test]
fn ledger_db_test() {
    use std::str::FromStr;

    let a = Address::from_str("eDFNpzpXw7CxMJo3Ez4mKaFF7AhnqtCosXcHMHpVVqBNtUys5").unwrap();
    let b = Address::from_str("jGYcEhE1ms5p8TfjPyKr456bkkLgdRFKqq7TLRGUPS8Tonfja").unwrap();

    let mut db = LedgerDB::new();
    let entry = LedgerEntry {
        parallel_balance: Amount::from_raw(42),
        ..Default::default()
    };
    db.put(&a, entry);
}
