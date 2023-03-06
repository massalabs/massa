use crate::types::ModuleInfo;
use massa_execution_exports::ExecutionError;
use massa_hash::Hash;
use massa_sc_runtime::{GasCosts, RuntimeModule};
use massa_serialization::{
    DeserializeError, Deserializer, OptionDeserializer, OptionSerializer, Serializer,
    U64VarIntDeserializer, U64VarIntSerializer,
};
use rocksdb::{ColumnFamily, ColumnFamilyDescriptor, Options, WriteBatch, DB};
use std::ops::Bound::Included;
use std::path::PathBuf;

const MODULE_CF: &str = "module";
const GAS_COSTS_CF: &str = "gas_cost";
const OPEN_ERROR: &str = "critical: rocksdb open operation failed";
const CF_ERROR: &str = "critical: rocksdb column family operation failed";
const GAS_COSTS_SER_ERR: &str = "gas cost serialization error";
const MOD_SER_ERR: &str = "module serialization error";
const KEY_NOT_FOUND: &str = "Key not found";

pub(crate) struct HDCache {
    db: DB,
    // How many entries are in the db. Count is initialized at creation time by iterating
    // over all the entries in the db then it is maintained in memory
    entry_count: usize,
    // Maximum number of entries we want to keep in the db.
    // When this maximum is reached `amount_to_snip` entries are removed
    max_entry_count: usize,
    // How many entries are removed when `entry_count` reaches `max_entry_count`
    amount_to_snip: usize,
    option_serializer: OptionSerializer<u64, U64VarIntSerializer>,
    option_deserializer: OptionDeserializer<u64, U64VarIntDeserializer>,
}

impl HDCache {
    /// Create a new HDCache
    ///
    /// # Arguments
    /// * path: where to store the db
    /// * max_entry_count: maximum number of entries we want to keep in the db
    /// * amount_to_remove: how many entries are removed when `entry_count` reaches `max_entry_count`
    pub fn new(path: PathBuf, max_entry_count: usize, amount_to_remove: usize) -> Self {
        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.create_missing_column_families(true);

        let db = DB::open_cf_descriptors(
            &db_opts,
            path,
            vec![
                ColumnFamilyDescriptor::new(MODULE_CF, Options::default()),
                ColumnFamilyDescriptor::new(GAS_COSTS_CF, Options::default()),
            ],
        )
        .expect(OPEN_ERROR);

        let entry_count = db
            .iterator_cf(
                db.cf_handle(GAS_COSTS_CF).expect(CF_ERROR),
                rocksdb::IteratorMode::Start,
            )
            .count();

        Self {
            db,
            entry_count,
            max_entry_count,
            amount_to_snip: amount_to_remove,
            option_serializer: OptionSerializer::new(U64VarIntSerializer::new()),
            option_deserializer: OptionDeserializer::new(U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(u64::MAX),
            )),
        }
    }

    /// Insert a new module in the cache
    pub fn insert(&mut self, hash: Hash, module_info: ModuleInfo) -> Result<(), ExecutionError> {
        let (module, init_cost) = module_info;

        let module = module
            .serialize()
            .map_err(|_| ExecutionError::RuntimeError(MOD_SER_ERR.into()))?;

        let mut gas_cost = vec![];
        self.option_serializer
            .serialize(&init_cost, &mut gas_cost)
            .expect(GAS_COSTS_SER_ERR);

        // if db is full do some cleaning
        if self.entry_count >= self.max_entry_count {
            self.snip(hash)?;
        }

        let mut batch = WriteBatch::default();

        for (cf, data) in [(self.module_cf(), module), (self.gas_costs_cf(), gas_cost)] {
            batch.put_cf(cf, hash.to_bytes(), data);
        }

        self.db
            .write(batch)
            .map_err(|err| ExecutionError::RuntimeError(err.to_string()))?;

        self.entry_count += 1;

        Ok(())
    }

    /// Sets the initialization cost of a given module separately
    ///
    /// # Arguments
    /// * `hash`: hash associated to the module we want to set the cost for
    ///      MUST exist else exit with error:
    ///        i.e. insert has been called before with the same hash and it has not been removed
    /// * `init_cost`: the new cost associated to the module
    pub fn set_init_cost(&self, hash: Hash, init_cost: u64) -> Result<(), ExecutionError> {
        // check that hash exists in the db
        self.db
            .get_cf(
                self.db.cf_handle(GAS_COSTS_CF).expect(CF_ERROR),
                hash.to_bytes(),
            )
            .map_err(|_| ExecutionError::RuntimeError(KEY_NOT_FOUND.to_string()))?;

        // serialize cost
        let mut gas_cost = vec![];
        self.option_serializer
            .serialize(&Some(init_cost), &mut gas_cost)
            .expect(GAS_COSTS_SER_ERR);

        // update db
        self.db
            .put_cf(self.gas_costs_cf(), hash.to_bytes(), &gas_cost)
            .map_err(|err| ExecutionError::RuntimeError(err.to_string()))?;

        Ok(())
    }

    /// Retrieve a module
    pub fn get(&self, hash: Hash, limit: u64, gas_costs: GasCosts) -> Option<ModuleInfo> {
        let Some(module) = self
            .db
            .get_cf(self.module_cf(), hash.to_bytes())
            .ok()
            .flatten() else{return None;};

        let Some(cost) = self
            .db
            .get_cf(self.gas_costs_cf(), hash.to_bytes())
            .ok()
            .flatten() else {return None;};

        let cost = self
            .option_deserializer
            .deserialize::<DeserializeError>(&cost)
            .ok()
            .and_then(|(_, cost)| cost);

        let module = RuntimeModule::deserialize(&module, limit, gas_costs).ok()?;

        Some((module, cost))
    }

    /// Try to remove as much as self.amount_to_snip entries from the db.
    /// Remove at least one entry
    fn snip(&mut self, hash: Hash) -> Result<(), ExecutionError> {
        // Should we use a rnd generated number here?
        let key = hash.to_bytes();

        let mut iter = self.db.raw_iterator_cf(self.module_cf());
        iter.seek_for_prev(key);

        let mut batch = WriteBatch::default();

        let mut snipped_entries_count = 0usize;
        loop {
            if !(iter.valid() && snipped_entries_count < self.amount_to_snip) {
                break;
            }
            // thanks to if above we can safely unwrap
            let key = iter.key().unwrap();

            for cf in [self.module_cf(), self.gas_costs_cf()] {
                batch.delete_cf(cf, key);
            }

            iter.next();
            snipped_entries_count += 1;
        }

        self.db
            .write(batch)
            .map_err(|err| ExecutionError::RuntimeError(err.to_string()))?;

        self.entry_count -= snipped_entries_count;

        Ok(())
    }

    fn module_cf(&self) -> &ColumnFamily {
        self.db.cf_handle(MODULE_CF).expect(CF_ERROR)
    }

    fn gas_costs_cf(&self) -> &ColumnFamily {
        self.db.cf_handle(GAS_COSTS_CF).expect(CF_ERROR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use massa_hash::Hash;
    use massa_sc_runtime::GasCosts;

    use std::path::PathBuf;

    use serial_test::serial;

    const TEST_DB_PATH: &str = "test_db";

    fn make_default_runtime_module() -> RuntimeModule {
        let bytecode: Vec<u8> = vec![
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x06, 0x01, 0x60, 0x01, 0x7f,
            0x01, 0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x0b, 0x01, 0x07, 0x61, 0x64, 0x64, 0x5f,
            0x6f, 0x6e, 0x65, 0x00, 0x00, 0x0a, 0x09, 0x01, 0x07, 0x00, 0x20, 0x00, 0x41, 0x01,
            0x6a, 0x0b, 0x00, 0x1a, 0x04, 0x6e, 0x61, 0x6d, 0x65, 0x01, 0x0a, 0x01, 0x00, 0x07,
            0x61, 0x64, 0x64, 0x5f, 0x6f, 0x6e, 0x65, 0x02, 0x07, 0x01, 0x00, 0x01, 0x00, 0x02,
            0x70, 0x30,
        ];

        RuntimeModule::new(&bytecode, 10, GasCosts::default(), true).unwrap()
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

        let module = make_default_runtime_module();

        let init_cost = 100;
        let limit = 1;
        let gas_costs = GasCosts::default();

        cache
            .insert(hash, (module.clone(), Some(init_cost)))
            .expect("insert should succeed");

        let (cached_module, cached_init_cost) = cache
            .get(hash, limit, gas_costs)
            .expect("get should succeed in test");

        assert_eq!(
            cached_module.serialize().unwrap(),
            module.serialize().unwrap()
        );
        assert_eq!(cached_init_cost.unwrap(), init_cost);
    }

    #[test]
    #[serial]
    fn test_insert_more_than_max_entry() {
        let mut cache = setup();

        let module = make_default_runtime_module();

        let init_cost = 100;

        // fill the db: add cache.max_entry_count entries
        for count in 0..cache.max_entry_count {
            let key = Hash::compute_from(count.to_string().as_bytes());
            cache
                .insert(key, (module.clone(), Some(init_cost)))
                .expect("insert should succeed");
        }
        assert_eq!(cache.entry_count, cache.max_entry_count);

        // insert one more entry
        let key = Hash::compute_from(cache.max_entry_count.to_string().as_bytes());
        cache
            .insert(key, (module.clone(), Some(init_cost)))
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
        let limit = 1;
        let gas_costs = GasCosts::default();

        cache
            .insert(hash, (make_default_runtime_module(), Some(init_cost)))
            .expect("insert should succeed");

        cache
            .set_init_cost(hash, init_cost)
            .expect("set init cost should succeed");

        let (_, cached_init_cost) = cache.get(hash, limit, gas_costs).unwrap();
        assert_eq!(cached_init_cost.unwrap(), init_cost);
    }
}
