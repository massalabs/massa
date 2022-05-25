// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Module to interact with the disk ledger

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

/// Disk ledger DB module
///
/// Contains a RocksDB DB instance
pub(crate) struct LedgerDB(pub(crate) DB);

/// Destroy the disk ledger db and free the lock
pub fn destroy_ledger_db() {
    DB::destroy(&Options::default(), DB_PATH).expect(OPEN_ERROR);
}

/// Balance key formatting macro
///
/// NOTE: ident being in front of addr is required for the intermediate bootstrap implementation
macro_rules! balance_key {
    ($addr:ident) => {
        [&[BALANCE_IDENT], &$addr.to_bytes()[..]].concat()
    };
}

/// Bytecode key formatting macro
///
/// NOTE: ident being in front of addr is required for the intermediate bootstrap implementation
/// NOTE: still handle separate bytecode for now to avoid too many refactoring at once
macro_rules! bytecode_key {
    ($addr:ident) => {
        [&[BYTECODE_IDENT], &$addr.to_bytes()[..]].concat()
    };
}

/// Datastore entry key formatting macro
///
/// TODO: add a separator identifier if the need comes to have multiple datastores
macro_rules! data_key {
    ($addr:ident, $key:ident) => {
        [&$addr.to_bytes()[..], &$key.to_bytes()[..]].concat()
    };
}

/// Datastore entry prefix formatting macro
macro_rules! data_prefix {
    ($addr:ident) => {
        &$addr.to_bytes()[..]
    };
}

/// Compute the end bound for a given prefix
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

// TODO: implement batch functions to have atomic updates of ledger
// TODO: save attached slot in metadata for a lighter bootstrap after disconnection
impl LedgerDB {
    /// Create and initialize a new LedgerDB
    pub fn new() -> Self {
        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.create_missing_column_families(true);

        let db = DB::open_cf_descriptors(
            &db_opts,
            DB_PATH,
            vec![
                ColumnFamilyDescriptor::new(LEDGER_CF, Options::default()),
                ColumnFamilyDescriptor::new(METADATA_CF, Options::default()),
            ],
        )
        .expect(OPEN_ERROR);

        LedgerDB(db)
    }

    /// Add every sub-entry of a given entry to the disk ledger
    ///
    /// # Arguments
    /// * addr: associated address
    /// * ledger_entry: complete entry to be added
    pub fn put_entry(&mut self, addr: &Address, ledger_entry: LedgerEntry) {
        let mut batch = WriteBatch::default();
        let handle = self.0.cf_handle(LEDGER_CF).expect(CF_ERROR);

        // balance
        batch.put_cf(
            handle,
            balance_key!(addr),
            // Amount::to_bytes_compact() never fails
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

    /// Get the given sub-entry of a given address
    ///
    /// # Arguments
    /// * addr: associated address
    /// * ty: type of the queried sub-entry
    ///
    /// # Returns
    /// An Option of the sub-entry value as bytes
    pub fn get_sub_entry(&self, addr: &Address, ty: LedgerSubEntry) -> Option<Vec<u8>> {
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

    /// Get every address and their corresponding balance
    /// This should only be used for debug purposes
    ///
    /// # Returns
    /// A BTreeMap with the addresse as key and the balance as value
    ///
    /// NOTE: Currently used in the intermediate bootstrap implementation
    pub fn get_every_address(&self) -> BTreeMap<Address, Amount> {
        let handle = self.0.cf_handle(LEDGER_CF).expect(CF_ERROR);

        let ledger = self
            .0
            .iterator_cf(handle, IteratorMode::Start)
            .collect::<Vec<_>>();

        let mut addresses = BTreeMap::new();
        for (key, entry) in ledger {
            if key.first() == Some(&BALANCE_IDENT) {
                addresses.insert(
                    Address::from_bytes(&key[1..].try_into().unwrap()).unwrap(),
                    Amount::from_bytes_compact(&entry).unwrap().0,
                );
            }
        }
        addresses
    }

    /// Get the entire datastore for a given address
    ///
    /// # Returns
    /// A BTreeMap with the entry hash as key and the data bytes as value
    pub fn get_entire_datastore(&self, addr: &Address) -> BTreeMap<Hash, Vec<u8>> {
        let handle = self.0.cf_handle(LEDGER_CF).expect(CF_ERROR);

        let mut opt = ReadOptions::default();
        opt.set_iterate_upper_bound(end_prefix(data_prefix!(addr)).unwrap());

        self.0
            .iterator_cf_opt(
                handle,
                opt,
                IteratorMode::From(data_prefix!(addr), Direction::Forward),
            )
            .map(|(key, data)| {
                (
                    Hash::from_bytes(key.split_at(HASH_SIZE_BYTES).1.try_into().unwrap()).unwrap(),
                    data.to_vec(),
                )
            })
            .collect()
    }

    /// Update the ledger entry of a given address
    ///
    /// # Arguments
    /// * entry_update: a descriptor of the entry updates to be applied
    pub fn update_entry(&mut self, addr: &Address, entry_update: LedgerEntryUpdate) {
        let mut batch = WriteBatch::default();
        let handle = self.0.cf_handle(LEDGER_CF).expect(CF_ERROR);

        // balance
        if let SetOrKeep::Set(balance) = entry_update.parallel_balance {
            batch.put_cf(
                handle,
                balance_key!(addr),
                // Amount::to_bytes_compact() never fails
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

    /// Delete every sub-entry associated to the given address
    pub fn delete_entry(&self, addr: &Address) {
        let handle = self.0.cf_handle(LEDGER_CF).expect(CF_ERROR);
        let mut batch = WriteBatch::default();

        // balance
        batch.delete_cf(handle, balance_key!(addr));

        // bytecode
        batch.delete_cf(handle, balance_key!(addr));

        // datastore
        let mut opt = ReadOptions::default();
        opt.set_iterate_upper_bound(end_prefix(data_prefix!(addr)).unwrap());
        for (key, _) in self.0.iterator_cf_opt(
            handle,
            opt,
            IteratorMode::From(data_prefix!(addr), Direction::Forward),
        ) {
            batch.delete_cf(handle, key);
        }

        // write batch
        self.0.write(batch).expect(CRUD_ERROR);
    }
}

/// Functional test of LedgerDB
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
    db.put_entry(&a, entry);
    db.update_entry(&a, entry_update);

    // asserts
    assert!(db.get_sub_entry(&a, LedgerSubEntry::Balance).is_some());
    assert_eq!(
        Amount::from_bytes_compact(&db.get_sub_entry(&a, LedgerSubEntry::Balance).unwrap())
            .unwrap()
            .0,
        Amount::from_raw(21)
    );
    assert_eq!(db.get_sub_entry(&b, LedgerSubEntry::Balance), None);
    assert_eq!(data, db.get_entire_datastore(&a));
    db.delete_entry(&a);
    assert!(db.get_sub_entry(&a, LedgerSubEntry::Balance).is_none());
    assert!(db.get_entire_datastore(&a).is_empty());
}
