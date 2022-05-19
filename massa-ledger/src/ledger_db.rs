// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_hash::Hash;
use massa_models::Address;
use rocksdb::{ColumnFamilyDescriptor, Options, WriteBatch, DB};

use crate::{ledger_changes::LedgerEntryUpdate, LedgerEntry, SetOrDelete, SetOrKeep};

const DB_PATH: &str = "../.db";
const LEDGER_CF: &str = "ledger";
const METADATA_CF: &str = "metadata";
const OPEN_ERROR: &str = "critical: rocksdb open operation failed";
const CRUD_ERROR: &str = "critical: rocksdb crud operation failed";
const CF_ERROR: &str = "critical: rocksdb column family operation failed";

/// Ledger sub entry enum
pub enum LedgerSubEntry {
    /// Balance
    Balance,
    /// Bytecode
    Bytecode,
    /// Datastore entry
    Datastore(Hash),
}

pub(crate) struct LedgerDB(DB);

// note: still handle separate bytecode for now to avoid too many refactoring at once
macro_rules! bytecode_key {
    ($addr:ident) => {
        format!("b:{}", $addr).as_bytes()
    };
}

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
                ColumnFamilyDescriptor::new(LEDGER_CF, Options::default()),
                ColumnFamilyDescriptor::new(METADATA_CF, Options::default()),
            ],
        )
        .expect(OPEN_ERROR);

        // return database
        LedgerDB(db)
    }

    pub fn put(&mut self, addr: &Address, ledger_entry: LedgerEntry) {
        let mut batch = WriteBatch::default();
        let key = addr.to_bytes();
        let handle = self.0.cf_handle(LEDGER_CF).expect(CF_ERROR);

        // balance
        batch.put_cf(
            handle,
            key,
            ledger_entry.parallel_balance.to_raw().to_be_bytes(),
        );

        // bytecode
        batch.put_cf(handle, bytecode_key!(addr), ledger_entry.bytecode);

        // datastore
        for (hash, entry) in ledger_entry.datastore {
            batch.put_cf(handle, data_key!(addr, hash), entry);
        }

        // write batch
        self.0.write(batch).expect(CRUD_ERROR);
    }

    pub fn update(&mut self, addr: &Address, entry_update: LedgerEntryUpdate) {
        let mut batch = WriteBatch::default();
        let key = addr.to_bytes();
        let handle = self.0.cf_handle(LEDGER_CF).expect(CF_ERROR);

        // balance
        if let SetOrKeep::Set(balance) = entry_update.parallel_balance {
            batch.put_cf(handle, key, balance.to_raw().to_be_bytes());
        }

        // bytecode
        if let SetOrKeep::Set(bytecode) = entry_update.bytecode {
            batch.put_cf(handle, bytecode_key!(addr), bytecode);
        }

        // datastore
        for (hash, update) in entry_update.datastore {
            match update {
                SetOrDelete::Set(entry) => batch.put_cf(handle, data_key!(addr, hash), entry),
                SetOrDelete::Delete => batch.delete_cf(handle, data_key!(addr, hash)),
            }
        }

        // write batch
        self.0.write(batch).expect(CRUD_ERROR);
    }

    pub fn delete(&self, _addr: &Address) {
        // note: define how we want to handle this first
    }

    pub fn entry_may_exist(&self, addr: &Address, ty: LedgerSubEntry) -> bool {
        let key = addr.to_bytes();
        let handle = self.0.cf_handle(LEDGER_CF).expect(CF_ERROR);

        match ty {
            LedgerSubEntry::Balance => self.0.key_may_exist_cf(handle, key),
            LedgerSubEntry::Bytecode => self.0.key_may_exist_cf(handle, bytecode_key!(addr)),
            LedgerSubEntry::Datastore(hash) => {
                self.0.key_may_exist_cf(handle, data_key!(addr, hash))
            }
        }
    }

    pub fn get_entry(&self, addr: &Address, ty: LedgerSubEntry) -> Option<Vec<u8>> {
        let key = addr.to_bytes();
        let handle = self.0.cf_handle(LEDGER_CF).expect(CF_ERROR);

        match ty {
            LedgerSubEntry::Balance => self.0.get_cf(handle, key).expect(CRUD_ERROR),
            LedgerSubEntry::Bytecode => self
                .0
                .get_cf(handle, bytecode_key!(addr))
                .expect(CRUD_ERROR),
            LedgerSubEntry::Datastore(hash) => self
                .0
                .get_cf(handle, data_key!(addr, hash))
                .expect(CRUD_ERROR),
        }
    }
}

#[test]
// note: test datastore handling as well
fn ledger_db_test() {
    use massa_models::Amount;
    use massa_signature::{derive_public_key, generate_random_private_key};

    let pub_a = derive_public_key(&generate_random_private_key());
    let pub_b = derive_public_key(&generate_random_private_key());
    let a = Address::from_public_key(&pub_a);
    let b = Address::from_public_key(&pub_b);

    let entry = LedgerEntry {
        parallel_balance: Amount::from_raw(42),
        ..Default::default()
    };
    let entry_update = LedgerEntryUpdate {
        parallel_balance: SetOrKeep::Set(Amount::from_raw(21)),
        bytecode: SetOrKeep::Keep,
        ..Default::default()
    };

    let mut db = LedgerDB::new();
    db.put(&a, entry);
    db.update(&a, entry_update);

    assert!(db.entry_may_exist(&a, LedgerSubEntry::Balance));
    assert_eq!(
        Amount::from_raw(u64::from_be_bytes(
            db.get_entry(&a, LedgerSubEntry::Balance)
                .unwrap()
                .try_into()
                .unwrap()
        )),
        Amount::from_raw(21)
    );
    assert_eq!(db.get_entry(&b, LedgerSubEntry::Balance), None);

    // note: add a delete after assert when it is implemented
}
