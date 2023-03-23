use crate::{
    error::CacheError,
    types::{ModuleInfo, ModuleMetadata, ModuleMetadataDeserializer, ModuleMetadataSerializer},
};
use massa_hash::Hash;
use massa_sc_runtime::{GasCosts, RuntimeModule};
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use rand::RngCore;
use rocksdb::{Direction, IteratorMode, WriteBatch, DB};
use std::path::PathBuf;

const OPEN_ERROR: &str = "critical: rocksdb open operation failed";
const CRUD_ERROR: &str = "critical: rocksdb crud operation failed";
const MD_SER_ERROR: &str = "critical: metadata serialization failed";
const MD_DESER_ERROR: &str = "critical: metadata deserialization failed";
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
    db: DB,
    /// How many entries are in the db. Count is initialized at creation time by iterating
    /// over all the entries in the db then it is maintained in memory
    entry_count: usize,
    /// Maximum number of entries we want to keep in the db.
    /// When this maximum is reached `amount_to_snip` entries are removed
    max_entry_count: usize,
    /// How many entries are removed when `entry_count` reaches `max_entry_count`
    amount_to_snip: usize,
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
    /// * amount_to_remove: how many entries are removed when `entry_count` reaches
    ///   `max_entry_count`
    pub fn new(path: PathBuf, max_entry_count: usize, amount_to_remove: usize) -> Self {
        let db = DB::open_default(path).expect(OPEN_ERROR);
        let entry_count = db.iterator(IteratorMode::Start).count();

        Self {
            db,
            entry_count,
            max_entry_count,
            amount_to_snip: amount_to_remove,
            meta_ser: ModuleMetadataSerializer::new(),
            meta_deser: ModuleMetadataDeserializer::new(),
        }
    }

    /// Insert a new module in the cache
    pub fn insert(&mut self, hash: Hash, module_info: ModuleInfo) -> Result<(), CacheError> {
        if self.entry_count >= self.max_entry_count {
            self.snip();
        }

        let mut ser_metadata = Vec::new();
        let ser_module = match module_info {
            ModuleInfo::Invalid => {
                self.meta_ser
                    .serialize(&ModuleMetadata::Invalid, &mut ser_metadata)
                    .expect(MD_SER_ERROR);
                Vec::new()
            }
            ModuleInfo::Module(module) => {
                self.meta_ser
                    .serialize(&ModuleMetadata::NotExecuted, &mut ser_metadata)
                    .expect(MD_SER_ERROR);
                module.serialize()?
            }
            ModuleInfo::ModuleAndDelta((module, delta)) => {
                self.meta_ser
                    .serialize(&ModuleMetadata::Delta(delta), &mut ser_metadata)
                    .expect(MD_SER_ERROR);
                module.serialize()?
            }
        };

        let mut batch = WriteBatch::default();
        batch.put(module_key!(hash), ser_module);
        batch.put(metadata_key!(hash), ser_metadata);
        self.db.write(batch).expect(CRUD_ERROR);

        self.entry_count = self.entry_count.saturating_add(1);

        Ok(())
    }

    /// Sets the initialization cost of a given module separately
    ///
    /// # Arguments
    /// * `hash`: hash associated to the module for which we want to set the cost MUST exist else
    ///   exit with error: i.e. insert has been called before with the same hash and it has not
    ///   been removed
    /// * `init_cost`: the new cost associated to the module
    pub fn set_init_cost(&self, hash: Hash, init_cost: u64) {
        let mut ser_metadata = Vec::new();
        self.meta_ser
            .serialize(&ModuleMetadata::Delta(init_cost), &mut ser_metadata)
            .expect(MD_SER_ERROR);
        self.db
            .put(metadata_key!(hash), ser_metadata)
            .expect(CRUD_ERROR);
    }

    /// Sets a given module as invalid
    pub fn set_invalid(&self, hash: Hash) {
        let mut ser_metadata = Vec::new();
        self.meta_ser
            .serialize(&ModuleMetadata::Invalid, &mut ser_metadata)
            .expect(MD_SER_ERROR);
        self.db
            .put(metadata_key!(hash), ser_metadata)
            .expect(CRUD_ERROR);
    }

    /// Retrieve a module
    pub fn get(
        &self,
        hash: Hash,
        limit: u64,
        gas_costs: GasCosts,
    ) -> Result<Option<ModuleInfo>, CacheError> {
        let mut iterator = self
            .db
            .iterator(IteratorMode::From(&module_key!(hash), Direction::Forward));
        // TODO: make sure of the missing object behaviour here
        if let (Some(Ok((_, ser_module))), Some(Ok((_, ser_metadata)))) =
            (iterator.next(), iterator.next())
        {
            let module = RuntimeModule::deserialize(&ser_module, limit, gas_costs)?;
            let (_, metadata) = self
                .meta_deser
                .deserialize::<DeserializeError>(&ser_metadata)
                .expect(MD_DESER_ERROR);
            let result = match metadata {
                ModuleMetadata::Invalid => ModuleInfo::Invalid,
                ModuleMetadata::NotExecuted => ModuleInfo::Module(module),
                ModuleMetadata::Delta(delta) => ModuleInfo::ModuleAndDelta((module, delta)),
            };
            Ok(Some(result))
        } else {
            Ok(None)
        }
    }

    /// Try to remove as much as self.amount_to_snip entries from the db.
    /// Remove at least one entry
    fn snip(&mut self) {
        let mut rbytes = Vec::new();
        rand::thread_rng().fill_bytes(&mut rbytes);

        // generate a key from the random number
        let key = *Hash::compute_from(&rbytes).to_bytes();

        let mut iter = self.db.raw_iterator();
        iter.seek_for_prev(key);

        let mut batch = WriteBatch::default();

        // prepare key for removal
        let mut snipped_entries_count = 0usize;
        loop {
            if !(iter.valid() && snipped_entries_count < self.amount_to_snip) {
                break;
            }
            // thanks to if above we can safely unwrap
            let key = iter.key().unwrap();

            batch.delete(key);

            iter.next();
            snipped_entries_count += 1;
        }

        self.db.write(batch).expect(CRUD_ERROR);

        self.entry_count -= snipped_entries_count;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use massa_hash::Hash;
    use massa_sc_runtime::{GasCosts, RuntimeModule};

    use std::path::PathBuf;

    use serial_test::serial;

    const TEST_DB_PATH: &str = "test_db";

    fn make_default_module_info() -> ModuleInfo {
        let bytecode: Vec<u8> = vec![
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x06, 0x01, 0x60, 0x01, 0x7f,
            0x01, 0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x0b, 0x01, 0x07, 0x61, 0x64, 0x64, 0x5f,
            0x6f, 0x6e, 0x65, 0x00, 0x00, 0x0a, 0x09, 0x01, 0x07, 0x00, 0x20, 0x00, 0x41, 0x01,
            0x6a, 0x0b, 0x00, 0x1a, 0x04, 0x6e, 0x61, 0x6d, 0x65, 0x01, 0x0a, 0x01, 0x00, 0x07,
            0x61, 0x64, 0x64, 0x5f, 0x6f, 0x6e, 0x65, 0x02, 0x07, 0x01, 0x00, 0x01, 0x00, 0x02,
            0x70, 0x30,
        ];

        ModuleInfo::Module(RuntimeModule::new(&bytecode, 10, GasCosts::default(), true).unwrap())
    }

    fn setup() -> HDCache {
        let _ = std::fs::remove_dir_all(TEST_DB_PATH);

        let path = PathBuf::from(TEST_DB_PATH);
        HDCache::new(path, 1000, 10)
    }

    #[test]
    #[serial]
    fn test_insert_and_get_simple() {
        let mut cache = setup();
        let hash = Hash::compute_from(b"test_hash");
        let module = make_default_module_info();

        let limit = 1;
        let gas_costs = GasCosts::default();

        cache
            .insert(hash, module.clone())
            .expect("insert should succeed");

        let _cached_module = cache
            .get(hash, limit, gas_costs)
            .expect("get should succeed in test");
    }

    #[test]
    #[serial]
    fn test_insert_more_than_max_entry() {
        let mut cache = setup();
        let module = make_default_module_info();

        // fill the db: add cache.max_entry_count entries
        for count in 0..cache.max_entry_count {
            let key = Hash::compute_from(count.to_string().as_bytes());
            cache
                .insert(key, module.clone())
                .expect("insert should succeed");
        }
        assert_eq!(cache.entry_count, cache.max_entry_count);

        // insert one more entry
        let key = Hash::compute_from(cache.max_entry_count.to_string().as_bytes());
        cache
            .insert(key, module.clone())
            .expect("insert should succeed");
        assert_eq!(
            cache.entry_count,
            cache.max_entry_count - cache.amount_to_snip + 1
        );

        dbg!(cache.entry_count);
    }

    #[test]
    #[serial]
    fn test_set_init_cost() {
        let mut cache = setup();
        let hash = Hash::compute_from(b"test_hash");
        let init_cost = 100;
        let gas_costs = GasCosts::default();

        cache
            .insert(hash, make_default_module_info())
            .expect("insert should succeed");

        cache.set_init_cost(hash, init_cost);

        // let (_, cached_init_cost) = cache.get(hash, limit, gas_costs).unwrap();
        // assert_eq!(cached_init_cost.unwrap(), init_cost);
    }
}
