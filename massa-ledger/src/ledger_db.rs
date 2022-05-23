// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_hash::{Hash, HASH_SIZE_BYTES};
use massa_models::{Address, Amount, DeserializeCompact, SerializeCompact};
use rocksdb::{
    ColumnFamilyDescriptor, Direction, IteratorMode, Options, ReadOptions, WriteBatch, DB,
};
use std::collections::BTreeMap;

use crate::{ledger_changes::LedgerEntryUpdate, LedgerEntry, SetOrDelete, SetOrKeep};

// TODO: remove rocks_db dir when sled is cut out
const DB_PATH: &str = "../massa-node/storage/ledger/rocks_db";
const LEDGER_CF: &str = "ledger";
const METADATA_CF: &str = "metadata";
const OPEN_ERROR: &str = "critical: rocksdb open operation failed";
const CRUD_ERROR: &str = "critical: rocksdb crud operation failed";
const CF_ERROR: &str = "critical: rocksdb column family operation failed";
const BALANCE_IDENT: u8 = 0u8;
const BYTECODE_IDENT: u8 = 1u8;

/// Ledger sub entry enum
pub enum LedgerSubEntry {
    /// Balance
    Balance,
    /// Bytecode
    Bytecode,
    /// Datastore entry
    Datastore(Hash),
}

pub(crate) struct LedgerDB(pub(crate) DB);

/// Destroy the disk ledger db and free the lock
pub fn destroy_ledger_db() {
    DB::destroy(&Options::default(), DB_PATH).expect(OPEN_ERROR);
}

macro_rules! balance_key {
    ($addr:ident) => {
        [&[BALANCE_IDENT], &$addr.to_bytes()[..]].concat()
    };
}

// NOTE: still handle separate bytecode for now to avoid too many refactoring at once
macro_rules! bytecode_key {
    ($addr:ident) => {
        [&[BYTECODE_IDENT], &$addr.to_bytes()[..]].concat()
    };
}

// TODO: add a separator identifier if the need comes to have multiple datastores
macro_rules! data_key {
    ($addr:ident, $key:ident) => {
        [&$addr.to_bytes()[..], &$key.to_bytes()[..]].concat()
    };
}

macro_rules! data_prefix {
    ($addr:ident) => {
        &$addr.to_bytes()[..]
    };
}

pub fn end_prefix(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut end_range = prefix.to_vec();
    while let Some(0xff) = end_range.last() {
        end_range.pop();
    }
    if let Some(byte) = end_range.last_mut() {
        *byte += 1;
        Some(end_range)
    } else {
        None
    }
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
        let handle = self.0.cf_handle(LEDGER_CF).expect(CF_ERROR);

        // balance
        batch.put_cf(
            handle,
            balance_key!(addr),
            // this unwrap will never fail
            ledger_entry.parallel_balance.to_bytes_compact().unwrap(),
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

    pub fn get_every_address(&self) -> BTreeMap<Address, Amount> {
        let handle = self.0.cf_handle(LEDGER_CF).expect(CF_ERROR);

        let ledger = self
            .0
            .iterator_cf(handle, IteratorMode::Start)
            .collect::<Vec<_>>();

        let mut addresses = BTreeMap::new();
        for (key, entry) in ledger {
            if &key[0] == &BALANCE_IDENT {
                addresses.insert(
                    Address::from_bytes(&key[1..].try_into().unwrap()).unwrap(),
                    Amount::from_bytes_compact(&entry).unwrap().0,
                );
            }
        }
        addresses
    }

    pub fn get_datastore_for(&self, addr: &Address) -> BTreeMap<Hash, Vec<u8>> {
        let handle = self.0.cf_handle(LEDGER_CF).expect(CF_ERROR);

        let mut opt = ReadOptions::default();
        opt.set_iterate_upper_bound(end_prefix(&data_prefix!(addr)).unwrap());

        self.0
            .iterator_cf_opt(
                handle,
                opt,
                IteratorMode::From(&data_prefix!(addr), Direction::Forward),
            )
            .collect::<Vec<_>>()
            .iter()
            .map(|(key, data)| {
                (
                    Hash::from_bytes(key.split_at(HASH_SIZE_BYTES).1.try_into().unwrap()).unwrap(),
                    data.to_vec(),
                )
            })
            .collect()
    }

    pub fn update(&mut self, addr: &Address, entry_update: LedgerEntryUpdate) {
        let mut batch = WriteBatch::default();
        let handle = self.0.cf_handle(LEDGER_CF).expect(CF_ERROR);

        // balance
        if let SetOrKeep::Set(balance) = entry_update.parallel_balance {
            batch.put_cf(
                handle,
                balance_key!(addr),
                // this unwrap will never fail
                balance.to_bytes_compact().unwrap(),
            );
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
        // TODO: define how we want to handle this first
    }

    pub fn entry_may_exist(&self, addr: &Address, ty: LedgerSubEntry) -> bool {
        let handle = self.0.cf_handle(LEDGER_CF).expect(CF_ERROR);

        match ty {
            LedgerSubEntry::Balance => self.0.key_may_exist_cf(handle, balance_key!(addr)),
            LedgerSubEntry::Bytecode => self.0.key_may_exist_cf(handle, bytecode_key!(addr)),
            LedgerSubEntry::Datastore(hash) => {
                self.0.key_may_exist_cf(handle, data_key!(addr, hash))
            }
        }
    }

    pub fn get_entry(&self, addr: &Address, ty: LedgerSubEntry) -> Option<Vec<u8>> {
        let handle = self.0.cf_handle(LEDGER_CF).expect(CF_ERROR);

        match ty {
            LedgerSubEntry::Balance => self.0.get_cf(handle, balance_key!(addr)).expect(CRUD_ERROR),
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
fn test_ledger_db() {
    use massa_models::Amount;
    use massa_signature::{derive_public_key, generate_random_private_key};

    // addresses
    let pub_a = derive_public_key(&generate_random_private_key());
    let pub_b = derive_public_key(&generate_random_private_key());
    let a = Address::from_public_key(&pub_a);
    let b = Address::from_public_key(&pub_b);

    // data
    let mut data = BTreeMap::new();
    data.insert(Hash::compute_from(b"1"), b"a".to_vec());
    data.insert(Hash::compute_from(b"2"), b"b".to_vec());
    data.insert(Hash::compute_from(b"3"), b"c".to_vec());
    let entry = LedgerEntry {
        parallel_balance: Amount::from_raw(42),
        datastore: data.clone(),
        ..Default::default()
    };
    let entry_update = LedgerEntryUpdate {
        parallel_balance: SetOrKeep::Set(Amount::from_raw(21)),
        bytecode: SetOrKeep::Keep,
        ..Default::default()
    };

    // db operations
    let mut db = LedgerDB::new();
    db.put(&a, entry);
    db.update(&a, entry_update);

    // asserts
    assert!(db.entry_may_exist(&a, LedgerSubEntry::Balance));
    assert_eq!(
        Amount::from_bytes_compact(&db.get_entry(&a, LedgerSubEntry::Balance).unwrap())
            .unwrap()
            .0,
        Amount::from_raw(21)
    );
    assert_eq!(db.get_entry(&b, LedgerSubEntry::Balance), None);
    assert_eq!(data, db.get_datastore_for(&a));

    // TODO: remove
    println!("{:#?}", db.get_every_address());

    // TODO: add a delete after assert when it is implemented
}
