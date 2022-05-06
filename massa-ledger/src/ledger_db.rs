use massa_hash::Hash;
use massa_models::{Address, Amount};
use rocksdb::{ColumnFamily, Options, WriteBatch, DB};
use std::collections::BTreeMap;

use crate::{ledger_changes::LedgerEntryUpdate, LedgerEntry, SetOrDelete, SetOrKeep};

const DB_PATH: &str = "_path_to_db";
const BALANCE_CF: &str = "balance";
const BYTECODE_CF: &str = "bytecode";
const OPEN_ERROR: &str = "critical: rocksdb open operation failed";
const CRUD_ERROR: &str = "critical: rocksdb crud operation failed";
const CF_ERROR: &str = "critical: rocksdb cf operation failed";

pub(crate) enum LedgerDBEntry {
    Balance,
    Bytecode,
    Datastore(Hash),
}

pub(crate) struct LedgerDB(DB);

// IMPORTANT NOTES:
// - find a way to open datastore cf's on new db
// - might not need to have a mutex on ledger with multi threaded disk db

macro_rules! balance_cf {
    ($self:ident) => {
        $self.0.cf_handle(BALANCE_CF).expect(CF_ERROR)
    };
}

macro_rules! bytecode_cf {
    ($self:ident) => {
        $self.0.cf_handle(BYTECODE_CF).expect(CF_ERROR)
    };
}

macro_rules! datastore_cf {
    ($self:ident, $name:ident) => {
        $self.0.cf_handle(&$name).expect(CF_ERROR)
    };
}

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
        LedgerDB(db)
    }

    // fn datastore_cf(&self, cf_name: String) -> ColumnFamily {
    //     match self.0.cf_handle(&cf_name) {
    //         Some(cf) => *cf,
    //         None => {
    //             self.0
    //                 .create_cf(cf_name, &Options::default())
    //                 .expect(CF_ERROR);
    //             *self.0.cf_handle(&cf_name).expect(CF_ERROR)
    //         }
    //     }
    // }

    pub fn put(&self, addr: &Address, ledger_entry: LedgerEntry) {
        let mut batch = WriteBatch::default();
        let key = addr.to_bytes();

        // balance
        batch.put_cf(
            balance_cf!(self),
            key,
            ledger_entry.parallel_balance.to_raw().to_be_bytes(),
        );

        // bytecode
        batch.put_cf(bytecode_cf!(self), key, ledger_entry.bytecode);

        // datastore
        let cf_name = addr.to_string();
        let cf = datastore_cf!(self, cf_name);
        for (hash, entry) in ledger_entry.datastore {
            let data_key = hash.to_bytes();
            batch.put_cf(&cf, data_key, entry);
        }
        self.0.write(batch).expect(CRUD_ERROR);
    }

    pub fn update(&self, addr: &Address, entry_update: LedgerEntryUpdate) {
        let mut batch = WriteBatch::default();
        let key = addr.to_bytes();

        // balance
        if let SetOrKeep::Set(balance) = entry_update.parallel_balance {
            batch.put_cf(balance_cf!(self), key, balance.to_raw().to_be_bytes());
        }

        // bytecode
        if let SetOrKeep::Set(bytecode) = entry_update.bytecode {
            batch.put_cf(bytecode_cf!(self), key, bytecode);
        }

        // datastore
        let cf_name = addr.to_string();
        let cf = datastore_cf!(self, cf_name);
        for (hash, update) in entry_update.datastore {
            let data_key = hash.to_bytes();
            match update {
                SetOrDelete::Set(entry) => batch.put_cf(&cf, data_key, entry),
                SetOrDelete::Delete => batch.delete_cf(&cf, data_key),
            }
        }
        self.0.write(batch).expect(CRUD_ERROR);
    }

    pub fn delete(&self, _addr: &Address) {
        // note: missing delete
    }

    pub fn entry_exists(&self, addr: &Address, ty: LedgerDBEntry) -> bool {
        let key = addr.to_bytes();
        let cf_name = addr.to_string();
        match ty {
            LedgerDBEntry::Balance => self.0.key_may_exist_cf(balance_cf!(self), key),
            LedgerDBEntry::Bytecode => self.0.key_may_exist_cf(bytecode_cf!(self), key),
            LedgerDBEntry::Datastore(hash) => self
                .0
                .key_may_exist_cf(datastore_cf!(self, cf_name), hash.to_bytes()),
        }
    }

    pub fn get_entry(&self, addr: &Address, ty: LedgerDBEntry) -> Option<Vec<u8>> {
        let key = addr.to_bytes();
        let cf_name = addr.to_string();
        match ty {
            LedgerDBEntry::Balance => self.0.get_cf(balance_cf!(self), key).expect(CRUD_ERROR),
            LedgerDBEntry::Bytecode => self.0.get_cf(bytecode_cf!(self), key).expect(CRUD_ERROR),
            LedgerDBEntry::Datastore(hash) => self
                .0
                .get_cf(datastore_cf!(self, cf_name), hash.to_bytes())
                .expect(CRUD_ERROR),
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
