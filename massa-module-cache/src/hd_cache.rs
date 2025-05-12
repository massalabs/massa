use crate::types::{
    ModuleInfo, ModuleMetadata, ModuleMetadataDeserializer, ModuleMetadataSerializer,
};
use massa_hash::Hash;
use massa_sc_runtime::{CondomLimits, GasCosts, RuntimeModule};
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use rand::RngCore;
use rocksdb::{Direction, IteratorMode, Options, WriteBatch, DB};
use std::path::PathBuf;
use tracing::{debug, warn};

const OPEN_ERROR: &str = "critical: rocksdb open operation failed";
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
        if path.exists() {
            if let Err(e) = DB::destroy(&Options::default(), path.clone()) {
                warn!("Failed to destroy the db: {:?}", e);
            }
        }
        let db = DB::open_default(path).expect(OPEN_ERROR);
        let entry_count = 0;

        Self {
            db: Some(db),
            entry_count,
            max_entry_count,
            snip_amount,
            meta_ser: ModuleMetadataSerializer::new(),
            meta_deser: ModuleMetadataDeserializer::new(),
        }
    }

    pub fn reset(&mut self) {
        let path = self.db.as_ref().unwrap().path().to_path_buf();

        // Close the existing database by dropping it
        let _ = self.db.take();

        // Destroy the database files
        if path.exists() {
            if let Err(e) = DB::destroy(&Options::default(), path.clone()) {
                warn!("Failed to destroy the db: {:?}", e);
            }
        }
        // Reopen the database
        let db = DB::open_default(&path).expect(OPEN_ERROR);
        self.db = Some(db);
        self.entry_count = 0;
    }

    /// Insert a new module in the cache
    pub fn insert(&mut self, hash: Hash, module_info: ModuleInfo) {
        if self.entry_count >= self.max_entry_count {
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
            .write(batch)
            .expect(CRUD_ERROR);

        self.entry_count = self.entry_count.saturating_add(1);

        debug!("(HD insert) entry_count is: {}", self.entry_count);
    }

    /// Sets the initialization cost of a given module separately
    ///
    /// # Arguments
    /// * `hash`: hash associated to the module for which we want to set the cost
    /// * `init_cost`: the new cost associated to the module
    pub fn set_init_cost(&self, hash: Hash, init_cost: u64) {
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

    /// Try to remove as much as `self.amount_to_snip` entries from the db
    fn snip(&mut self) {
        let mut iter = self.db.as_ref().expect(CRUD_ERROR).raw_iterator();
        let mut batch = WriteBatch::default();
        let mut snipped_count: usize = 0;

        while snipped_count < self.snip_amount {
            // generate a random key
            let mut rbytes = [0u8; 16];
            rand::thread_rng().fill_bytes(&mut rbytes);
            let key = *Hash::compute_from(&rbytes).to_bytes();

            // take the upper existing key
            iter.seek_for_prev(key);

            // check iterator validity
            if !iter.valid() {
                continue;
            }

            // unwrap justified by above conditional statement.
            // seeking the previous key of a randombly generated one
            // will always end up on a metadata key.
            let metadata_key = iter.key().unwrap();
            batch.delete(metadata_key);
            iter.prev();
            let module_key = iter.key().unwrap();
            batch.delete(module_key);

            // increase snipped_count
            snipped_count += 1;
        }

        // safety check
        if batch.len() / 2 != snipped_count {
            panic!("snipped_count incoherence");
        }

        // delete the key and reduce entry_count
        self.db
            .as_ref()
            .expect(CRUD_ERROR)
            .write(batch)
            .expect(CRUD_ERROR);
        self.entry_count -= snipped_count;
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
