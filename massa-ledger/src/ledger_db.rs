// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_hash::Hash;
use massa_models::{Address, Amount};
use rocksdb::{ColumnFamilyDescriptor, Options, WriteBatch, DB};
use std::collections::BTreeMap;

use crate::{ledger_changes::LedgerEntryUpdate, LedgerEntry, SetOrDelete, SetOrKeep};

const DB_PATH: &str = "../.db";
const BALANCE_CF: &str = "balance";
const BYTECODE_CF: &str = "bytecode";
const DATASTORE_CF: &str = "datastore";
const OPEN_ERROR: &str = "critical: rocksdb open operation failed";
const CRUD_ERROR: &str = "critical: rocksdb crud operation failed";
const CF_ERROR: &str = "critical: rocksdb column family operation failed";

pub(crate) enum LedgerDBEntry {
    Balance,
    Bytecode,
    Datastore(Hash),
}

pub(crate) struct LedgerDB(DB);

macro_rules! data_key {
    ($addr:ident, $key:ident) => {
        format!("{}:{}", $addr, $key).as_bytes()
    };
}

impl LedgerDB {
    pub fn new() -> Self {
        // db options
        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.create_missing_column_families(true);

        // database init
        let db = DB::open_cf_descriptors(
            &db_opts,
            DB_PATH,
            vec![
                ColumnFamilyDescriptor::new(BALANCE_CF, Options::default()),
                ColumnFamilyDescriptor::new(BYTECODE_CF, Options::default()),
                ColumnFamilyDescriptor::new(DATASTORE_CF, Options::default()),
            ],
        )
        .expect(OPEN_ERROR);

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
        let data_cf = self.0.cf_handle(DATASTORE_CF).expect(CF_ERROR);
        for (hash, entry) in ledger_entry.datastore {
            batch.put_cf(data_cf, data_key!(addr, hash), entry);
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
        let data_cf = self.0.cf_handle(DATASTORE_CF).expect(CF_ERROR);
        for (hash, update) in entry_update.datastore {
            match update {
                SetOrDelete::Set(entry) => batch.put_cf(&data_cf, data_key!(addr, hash), entry),
                SetOrDelete::Delete => batch.delete_cf(&data_cf, data_key!(addr, hash)),
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
    assert!(db.entry_exists(&a, LedgerDBEntry::Balance));
    assert!(!db.entry_exists(&b, LedgerDBEntry::Balance));
}
