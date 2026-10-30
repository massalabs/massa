use crate::types::{
    ModuleInfo, ModuleMetadata, ModuleMetadataDeserializer, ModuleMetadataSerializer,
};
use massa_hash::{Hash, HASH_SIZE_BYTES};
use massa_sc_runtime::{CondomLimits, GasCosts, RuntimeModule};
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use rand::RngCore;
use rocksdb::{Direction, IteratorMode, Options, WriteBatch, DB};
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

const OPEN_ERROR: &str = "critical: rocksdb open operation failed";
const DESTROY_ERROR: &str = "critical: could not remove the previous module cache db";
const CRUD_ERROR: &str = "critical: rocksdb crud operation failed";
const DATA_SER_ERROR: &str = "critical: metadata serialization failed";
const DATA_DESER_ERROR: &str = "critical: metadata deserialization failed";
const MOD_SER_ERROR: &str = "critical: module serialization failed";
const MOD_DESER_ERROR: &str = "critical: module deserialization failed";
const MODULE_IDENT: u8 = 0u8;
const DATA_IDENT: u8 = 1u8;

/// Module key formatting macro
#[macro_export]
macro_rules! module_key {
    ($bc_hash:expr) => {
        [&$bc_hash.to_bytes()[..], &[MODULE_IDENT]].concat()
    };
}

/// Delta key formatting macro
#[macro_export]
macro_rules! metadata_key {
    ($bc_hash:expr) => {
        [&$bc_hash.to_bytes()[..], &[DATA_IDENT]].concat()
    };
}

/// Remove the db at `path` if there is one, so that the cache always starts empty.
///
/// `entry_count` starts at 0, so silently reopening a db that could not be destroyed would
/// reuse stale entries (possibly left in a bad state) that are not accounted for.
fn destroy_db(path: &Path) {
    if !path.exists() {
        return;
    }
    if let Err(e) = DB::destroy(&Options::default(), path) {
        warn!(
            "Failed to destroy the db: {:?}, removing its directory instead",
            e
        );
        std::fs::remove_dir_all(path).expect(DESTROY_ERROR);
    }
}

pub(crate) struct HDCache {
    /// RocksDB database
    db: Option<DB>,
    /// How many entries are in the db. Count is initialized at creation time by iterating
    /// over all the entries in the db then it is maintained in memory
    entry_count: usize,
    /// Maximum number of entries we want to keep in the db.
    /// When this maximum is reached `snip_amount` entries are removed
    max_entry_count: usize,
    /// How many entries are removed when `entry_count` reaches `max_entry_count`
    snip_amount: usize,
    /// Module metadata serializer
    meta_ser: ModuleMetadataSerializer,
    /// Module metadata deserializer
    meta_deser: ModuleMetadataDeserializer,
    /// Test-only counter of `get` calls, used to assert that no redundant
    /// RocksDB read/deserialize happens when a module is already in the LRU cache.
    #[cfg(test)]
    pub(crate) read_count: std::sync::atomic::AtomicUsize,
}

impl HDCache {
    /// Create a new HDCache
    ///
    /// # Arguments
    /// * path: where to store the db
    /// * max_entry_count: maximum number of entries we want to keep in the db
    /// * amount_to_remove: how many entries are removed when `entry_count` reaches `max_entry_count`
    pub fn new(path: PathBuf, max_entry_count: usize, snip_amount: usize) -> Self {
        // Reset the DB if it already exists
        destroy_db(&path);
        let db = DB::open_default(path).expect(OPEN_ERROR);
        let entry_count = 0;

        Self {
            db: Some(db),
            entry_count,
            max_entry_count,
            snip_amount,
            meta_ser: ModuleMetadataSerializer::new(),
            meta_deser: ModuleMetadataDeserializer::new(),
            #[cfg(test)]
            read_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub fn reset(&mut self) {
        let path = self.db.as_ref().unwrap().path().to_path_buf();

        // Close the existing database by dropping it
        let _ = self.db.take();

        // Destroy the database files
        destroy_db(&path);
        // Reopen the database
        let db = DB::open_default(&path).expect(OPEN_ERROR);
        self.db = Some(db);
        self.entry_count = 0;
    }

    /// Whether a module is stored for `hash`.
    /// A module key is always written together with its metadata key, see `insert`.
    fn contains(&self, hash: Hash) -> bool {
        self.db
            .as_ref()
            .expect(CRUD_ERROR)
            .get_pinned(module_key!(hash))
            .expect(CRUD_ERROR)
            .is_some()
    }

    /// Insert a new module in the cache, or overwrite an already stored one
    pub fn insert(&mut self, hash: Hash, module_info: ModuleInfo) {
        // only count new entries, so that `entry_count` matches the stored modules
        let is_new = !self.contains(hash);
        if is_new && self.entry_count >= self.max_entry_count {
            self.snip();
        }

        let mut ser_metadata = Vec::new();
        let ser_module = match module_info {
            ModuleInfo::Invalid(err_msg) => {
                self.meta_ser
                    .serialize(&ModuleMetadata::Invalid(err_msg), &mut ser_metadata)
                    .expect(DATA_SER_ERROR);
                Vec::new()
            }
            ModuleInfo::Module(module) => {
                self.meta_ser
                    .serialize(&ModuleMetadata::NotExecuted, &mut ser_metadata)
                    .expect(DATA_SER_ERROR);
                module.serialize().expect(MOD_SER_ERROR)
            }
            ModuleInfo::ModuleAndDelta((module, delta)) => {
                self.meta_ser
                    .serialize(&ModuleMetadata::Delta(delta), &mut ser_metadata)
                    .expect(DATA_SER_ERROR);
                module.serialize().expect(MOD_SER_ERROR)
            }
        };

        let mut batch = WriteBatch::default();
        batch.put(module_key!(hash), ser_module);
        batch.put(metadata_key!(hash), ser_metadata);
        self.db
            .as_ref()
            .expect(CRUD_ERROR)
            .write(&batch)
            .expect(CRUD_ERROR);

        if is_new {
            self.entry_count = self.entry_count.saturating_add(1);
        }

        debug!("(HD insert) entry_count is: {}", self.entry_count);
    }

    /// Sets the initialization cost of a given module separately
    ///
    /// # Arguments
    /// * `hash`: hash associated to the module for which we want to set the cost
    /// * `init_cost`: the new cost associated to the module
    pub fn set_init_cost(&self, hash: Hash, init_cost: u64) {
        // The module may be absent (snipped, or only in the LRU cache): writing its metadata
        // alone would leave an orphan key, so skip it.
        if !self.contains(hash) {
            return;
        }
        let mut ser_metadata = Vec::new();
        self.meta_ser
            .serialize(&ModuleMetadata::Delta(init_cost), &mut ser_metadata)
            .expect(DATA_SER_ERROR);
        self.db
            .as_ref()
            .expect(CRUD_ERROR)
            .put(metadata_key!(hash), ser_metadata)
            .expect(CRUD_ERROR);
    }

    /// Sets a given module as invalid
    pub fn set_invalid(&self, hash: Hash, err_msg: String) {
        // see `set_init_cost`
        if !self.contains(hash) {
            return;
        }
        let mut ser_metadata = Vec::new();
        self.meta_ser
            .serialize(&ModuleMetadata::Invalid(err_msg), &mut ser_metadata)
            .expect(DATA_SER_ERROR);
        self.db
            .as_ref()
            .expect(CRUD_ERROR)
            .put(metadata_key!(hash), ser_metadata)
            .expect(CRUD_ERROR);
    }

    /// Retrieve a module
    pub fn get(
        &self,
        hash: Hash,
        gas_costs: GasCosts,
        condom_limits: CondomLimits,
    ) -> Option<ModuleInfo> {
        #[cfg(test)]
        self.read_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let mut iterator = self
            .db
            .as_ref()
            .expect(CRUD_ERROR)
            .iterator(IteratorMode::From(&module_key!(hash), Direction::Forward));

        if let (Some(Ok((key_1, ser_module))), Some(Ok((key_2, ser_metadata)))) =
            (iterator.next(), iterator.next())
        {
            if *key_1 == module_key!(hash) && *key_2 == metadata_key!(hash) {
                let (_, metadata) = self
                    .meta_deser
                    .deserialize::<DeserializeError>(&ser_metadata)
                    .expect(DATA_DESER_ERROR);
                if let ModuleMetadata::Invalid(err_msg) = metadata {
                    return Some(ModuleInfo::Invalid(err_msg));
                }
                let module = RuntimeModule::deserialize(
                    &ser_module,
                    gas_costs.max_instance_cost,
                    gas_costs,
                    condom_limits,
                )
                .expect(MOD_DESER_ERROR);
                let result = match metadata {
                    ModuleMetadata::Invalid(err_msg) => ModuleInfo::Invalid(err_msg),
                    ModuleMetadata::NotExecuted => ModuleInfo::Module(module),
                    ModuleMetadata::Delta(delta) => ModuleInfo::ModuleAndDelta((module, delta)),
                };
                Some(result)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Try to remove as much as `self.snip_amount` entries from the db.
    ///
    /// Walks the keys from a random point of the key space, wrapping around once, and removes
    /// every visited key. It does not assume that module and metadata keys come in pairs, so a
    /// stray key is removed instead of crashing the node, and only removed modules are counted.
    fn snip(&mut self) {
        let db = self.db.as_ref().expect(CRUD_ERROR);
        let mut batch = WriteBatch::default();
        let mut snipped_count: usize = 0;

        // random starting point
        let mut rbytes = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut rbytes);
        let start = *Hash::compute_from(&rbytes).to_bytes();

        let mut iter = db.raw_iterator();
        iter.seek(start);
        let mut wrapped = false;
        while snipped_count < self.snip_amount {
            let Some(key) = iter.key() else {
                // end of the db (or read error): wrap around once
                iter.status().expect(CRUD_ERROR);
                if wrapped {
                    break;
                }
                wrapped = true;
                iter.seek_to_first();
                continue;
            };
            if wrapped && key >= &start[..] {
                // every key has been visited
                break;
            }

            batch.delete(key);
            if key.len() == HASH_SIZE_BYTES + 1 && key[HASH_SIZE_BYTES] == MODULE_IDENT {
                // its metadata key comes right after: delete it now in case we stop here
                let mut metadata_key = key.to_vec();
                metadata_key[HASH_SIZE_BYTES] = DATA_IDENT;
                batch.delete(metadata_key);
                snipped_count += 1;
            }
            iter.next();
        }

        // delete the keys and reduce entry_count
        db.write(&batch).expect(CRUD_ERROR);
        self.entry_count = self.entry_count.saturating_sub(snipped_count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use massa_hash::Hash;
    use massa_sc_runtime::{Compiler, GasCosts, RuntimeModule};
    use rand::thread_rng;
    use serial_test::serial;
    use tempfile::TempDir;

    fn make_default_module_info() -> ModuleInfo {
        let bytecode: Vec<u8> = vec![
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x06, 0x01, 0x60, 0x01, 0x7f,
            0x01, 0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x0b, 0x01, 0x07, 0x61, 0x64, 0x64, 0x5f,
            0x6f, 0x6e, 0x65, 0x00, 0x00, 0x0a, 0x09, 0x01, 0x07, 0x00, 0x20, 0x00, 0x41, 0x01,
            0x6a, 0x0b, 0x00, 0x1a, 0x04, 0x6e, 0x61, 0x6d, 0x65, 0x01, 0x0a, 0x01, 0x00, 0x07,
            0x61, 0x64, 0x64, 0x5f, 0x6f, 0x6e, 0x65, 0x02, 0x07, 0x01, 0x00, 0x01, 0x00, 0x02,
            0x70, 0x30,
        ];
        ModuleInfo::Module(
            RuntimeModule::new(
                &bytecode,
                GasCosts::default(),
                Compiler::CL,
                CondomLimits::default(),
            )
            .unwrap(),
        )
    }

    fn setup() -> HDCache {
        let tmp_path = TempDir::new().unwrap().path().to_path_buf();
        HDCache::new(tmp_path, 1000, 10)
    }

    #[test]
    #[serial]
    fn test_basic_crud() {
        let mut cache = setup();
        let hash = Hash::compute_from(b"test_hash");
        let module = make_default_module_info();

        let init_cost = 100;
        let gas_costs = GasCosts::default();
        let condom_limits = CondomLimits::default();

        cache.insert(hash, module);
        let cached_module_v1 = cache
            .get(hash, gas_costs.clone(), condom_limits.clone())
            .unwrap();
        assert!(matches!(cached_module_v1, ModuleInfo::Module(_)));

        cache.set_init_cost(hash, init_cost);
        let cached_module_v2 = cache
            .get(hash, gas_costs.clone(), condom_limits.clone())
            .unwrap();
        assert!(matches!(cached_module_v2, ModuleInfo::ModuleAndDelta(_)));

        let err_msg = "test_error".to_string();
        cache.set_invalid(hash, err_msg.clone());
        let cached_module_v3 = cache.get(hash, gas_costs, condom_limits.clone()).unwrap();
        let ModuleInfo::Invalid(res_err) = cached_module_v3 else {
            panic!("expected ModuleInfo::Invalid");
        };
        assert_eq!(res_err, err_msg);
    }

    #[test]
    #[serial]
    fn test_insert_more_than_max_entry() {
        let mut cache = setup();
        let module = make_default_module_info();

        // fill the db: add cache.max_entry_count entries
        for count in 0..cache.max_entry_count {
            let key = Hash::compute_from(count.to_string().as_bytes());
            cache.insert(key, module.clone());
        }
        assert_eq!(cache.entry_count, cache.max_entry_count);

        // insert one more entry
        let key = Hash::compute_from(cache.max_entry_count.to_string().as_bytes());
        cache.insert(key, module);
        assert_eq!(
            cache.entry_count,
            cache.max_entry_count - cache.snip_amount + 1
        );
        dbg!(cache.entry_count);
    }

    /// Number of module keys stored in the db, i.e. the actual entry count
    fn stored_module_count(cache: &HDCache) -> usize {
        cache
            .db
            .as_ref()
            .unwrap()
            .iterator(IteratorMode::Start)
            .filter(|kv| {
                let (key, _) = kv.as_ref().unwrap();
                key.len() == HASH_SIZE_BYTES + 1 && key[HASH_SIZE_BYTES] == MODULE_IDENT
            })
            .count()
    }

    #[test]
    #[serial]
    fn test_metadata_update_of_missing_module_is_ignored() {
        let cache = setup();
        let hash = Hash::compute_from(b"never_inserted");

        cache.set_init_cost(hash, 100);
        cache.set_invalid(hash, "test_error".to_string());

        // no orphan metadata key is written
        let mut iter = cache.db.as_ref().unwrap().iterator(IteratorMode::Start);
        assert!(iter.next().is_none());
    }

    #[test]
    #[serial]
    fn test_overwrite_does_not_grow_entry_count() {
        let mut cache = setup();
        let hash = Hash::compute_from(b"test_hash");
        let module = make_default_module_info();

        cache.insert(hash, module.clone());
        cache.insert(hash, module);
        assert_eq!(cache.entry_count, 1);
        assert_eq!(stored_module_count(&cache), 1);
    }

    #[test]
    #[serial]
    fn test_snip_survives_stray_keys() {
        let tmp_path = TempDir::new().unwrap().path().to_path_buf();
        let mut cache = HDCache::new(tmp_path, 20, 5);
        let module = make_default_module_info();

        // stray keys: an orphan metadata key that is the smallest possible key (the previous
        // implementation stepped back from it past the start of the db and panicked), and a key
        // that does not follow the cache layout
        let db = cache.db.as_ref().unwrap();
        db.put(metadata_key!(Hash::from_bytes(&[0u8; 32])), [0u8])
            .unwrap();
        db.put([0xAAu8; 7], [0u8]).unwrap();

        for count in 0..500 {
            let key = Hash::compute_from(count.to_string().as_bytes());
            cache.insert(key, module.clone());
            assert!(cache.entry_count <= cache.max_entry_count);
            assert_eq!(cache.entry_count, stored_module_count(&cache));
        }
    }

    #[test]
    #[serial]
    fn test_new_and_reset_start_from_an_empty_db() {
        let tmp_dir = TempDir::new().unwrap();
        let path = tmp_dir.path().join("hd_cache");
        let hash = Hash::compute_from(b"test_hash");
        let gas_costs = GasCosts::default();
        let condom_limits = CondomLimits::default();
        let is_empty = |cache: &HDCache| {
            let mut iter = cache.db.as_ref().unwrap().iterator(IteratorMode::Start);
            iter.next().is_none()
        };

        // a previous run left an entry and a stray key on disk
        {
            let mut cache = HDCache::new(path.clone(), 1000, 10);
            cache.insert(hash, make_default_module_info());
            cache.db.as_ref().unwrap().put([0xAAu8; 7], [0u8]).unwrap();
        }

        // reopening starts from scratch
        let mut cache = HDCache::new(path, 1000, 10);
        assert!(is_empty(&cache));
        assert!(cache
            .get(hash, gas_costs.clone(), condom_limits.clone())
            .is_none());

        // so does a reset
        cache.insert(hash, make_default_module_info());
        cache.reset();
        assert!(is_empty(&cache));
        assert_eq!(cache.entry_count, 0);
        assert!(cache.get(hash, gas_costs, condom_limits).is_none());
    }

    #[test]
    #[serial]
    fn test_snip_amount_larger_than_entry_count() {
        let tmp_path = TempDir::new().unwrap().path().to_path_buf();
        let mut cache = HDCache::new(tmp_path, 3, 10);
        let module = make_default_module_info();

        for count in 0..10 {
            let key = Hash::compute_from(count.to_string().as_bytes());
            cache.insert(key, module.clone());
            assert_eq!(cache.entry_count, stored_module_count(&cache));
        }
    }

    #[test]
    #[serial]
    fn test_missing_module() {
        let mut cache = setup();
        let module = make_default_module_info();

        let gas_costs = GasCosts::default();
        let condom_limits = CondomLimits::default();

        for count in 0..cache.max_entry_count {
            let key = Hash::compute_from(count.to_string().as_bytes());
            cache.insert(key, module.clone());
        }

        for _ in 0..cache.max_entry_count {
            let mut rbytes = [0u8; 16];
            thread_rng().fill_bytes(&mut rbytes);
            let get_key = Hash::compute_from(&rbytes);
            let cached_module = cache.get(get_key, gas_costs.clone(), condom_limits.clone());
            assert!(cached_module.is_none());
        }
    }
}
