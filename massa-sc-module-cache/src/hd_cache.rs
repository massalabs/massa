use massa_execution_exports::ExecutionError;
use massa_hash::Hash;
use massa_sc_runtime::{GasCosts, RuntimeModule};
use massa_serialization::{
    DeserializeError, Deserializer, OptionDeserializer, OptionSerializer, Serializer,
    U64VarIntDeserializer, U64VarIntSerializer,
};
use rocksdb::{ColumnFamily, ColumnFamilyDescriptor, Options, DB};
use std::ops::Bound::Included;
use std::path::PathBuf;

const MODULE_CF: &str = "module";
const GAS_COSTS_CF: &str = "gas_cost";
const REF_COUNT_CF: &str = "ref_count";
const OPEN_ERROR: &str = "critical: rocksdb open operation failed";
const CF_ERROR: &str = "critical: rocksdb column family operation failed";
const GAS_COSTS_SER_ERR: &str = "gas cost serialization error";
const MOD_SER_ERR: &str = "module serialization error";
const KEY_NOT_FOUND: &str = "Key not found";
const REF_COUNT_NOT_FOUND: &str = "ref count in HD cache can't be None";

use crate::types::ModuleInfo;

pub(crate) struct HDCache {
    db: DB,
    u64_serializer: U64VarIntSerializer,
    u64_deserializer: U64VarIntDeserializer,
    option_serializer: OptionSerializer<u64, U64VarIntSerializer>,
}

impl HDCache {
    /// Create a new HDCache
    pub fn new(path: PathBuf) -> Self {
        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.create_missing_column_families(true);

        let db = DB::open_cf_descriptors(
            &db_opts,
            path,
            vec![
                ColumnFamilyDescriptor::new(MODULE_CF, Options::default()),
                ColumnFamilyDescriptor::new(GAS_COSTS_CF, Options::default()),
                ColumnFamilyDescriptor::new(REF_COUNT_CF, Options::default()),
            ],
        )
        .expect(OPEN_ERROR);

        Self {
            db,
            u64_serializer: U64VarIntSerializer::new(),
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
            option_serializer: OptionSerializer::new(U64VarIntSerializer::new()),
        }
    }

    /// Insert a new module in the cache
    pub fn insert(&self, hash: Hash, module_info: ModuleInfo) -> Result<(), ExecutionError> {
        let (module, init_cost) = module_info;

        let module = module
            .serialize()
            .map_err(|_| ExecutionError::RuntimeError(MOD_SER_ERR.into()))?;

        let mut gas_cost = vec![];
        self.option_serializer
            .serialize(&init_cost, &mut gas_cost)
            .expect(GAS_COSTS_SER_ERR);

        let mut ref_count = vec![];
        self.u64_serializer
            .serialize(&1u64, &mut ref_count)
            .expect(GAS_COSTS_SER_ERR);

        self.db
            .put_cf(self.module_cf(), hash.to_bytes(), &module)
            .map_err(|err| ExecutionError::RuntimeError(err.to_string()))?;

        self.db
            .put_cf(self.gas_costs_cf(), hash.to_bytes(), &gas_cost)
            .map_err(|err| ExecutionError::RuntimeError(err.to_string()))?;

        self.db
            .put_cf(self.ref_count_cf(), hash.to_bytes(), &ref_count)
            .map_err(|err| ExecutionError::RuntimeError(err.to_string()))?;

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
            .put_cf(
                self.db.cf_handle(GAS_COSTS_CF).expect(CF_ERROR),
                hash.to_bytes(),
                &gas_cost,
            )
            .map_err(|err| ExecutionError::RuntimeError(err.to_string()))?;

        Ok(())
    }

    /// Retrieve a module and increment its associated reference counter
    ///
    /// # Returns
    /// None if either a module of its cost has not been found
    /// Some((module, cost)) if both the module and its cost have been found.
    /// In this case the associated ref count is incremented.
    pub fn get_and_increment(
        &self,
        hash: Hash,
        limit: u64,
        gas_costs: GasCosts,
    ) -> Option<ModuleInfo> {
        match self.get(hash, limit, gas_costs) {
            Some(value) => {
                let ref_count = self.get_ref_count(hash)? + 1u64;
                self.set_ref_count(hash, ref_count).ok()?;

                Some(value)
            }
            None => None,
        }
    }

    fn set_ref_count(&self, hash: Hash, ref_count: u64) -> Result<(), ExecutionError> {
        let mut buffer = vec![];
        self.u64_serializer
            .serialize(&ref_count, &mut buffer)
            .expect(GAS_COSTS_SER_ERR);
        self.db
            .put_cf(self.ref_count_cf(), hash.to_bytes(), &buffer)
            .map_err(|err| ExecutionError::RuntimeError(err.into_string()))
    }

    fn get_ref_count(&self, hash: Hash) -> Option<u64> {
        let ref_count = self
            .db
            .get_cf(self.ref_count_cf(), hash.to_bytes())
            .ok()
            .flatten()
            .expect(REF_COUNT_NOT_FOUND);
        let (_, ref_count) = self
            .u64_deserializer
            .deserialize::<DeserializeError>(&ref_count)
            .ok()?;
        Some(ref_count)
    }

    /// Retrieve a module
    pub fn get(&self, hash: Hash, limit: u64, gas_costs: GasCosts) -> Option<ModuleInfo> {
        let module = self
            .db
            .get_cf(self.module_cf(), hash.to_bytes())
            .ok()
            .flatten();

        let cost = self
            .db
            .get_cf(self.gas_costs_cf(), hash.to_bytes())
            .ok()
            .flatten();

        let cost = match cost {
            Some(value) => self
                .u64_deserializer
                .deserialize::<DeserializeError>(&value)
                .ok()
                .map(|(_, cost)| Some(cost))
                .flatten(),
            None => None,
        };

        if let Some(module) = module {
            let module = RuntimeModule::deserialize(&module, limit, gas_costs).ok()?;
            Some((module, cost))
        } else {
            None
        }
    }

    /// Remove a module
    ///
    /// On call
    /// 1. decrease internal ref_count
    /// 2  if ref_count reaches 0 then remove data associated to the given hash
    pub fn remove(&self, hash: Hash) -> Result<(), ExecutionError> {
        let ref_count = self
            .get_ref_count(hash)
            .ok_or(ExecutionError::RuntimeError(REF_COUNT_NOT_FOUND.into()))?;

        if ref_count > 1 {
            return self.set_ref_count(hash, ref_count - 1);
        }

        // remove all
        for cf in [self.module_cf(), self.gas_costs_cf(), self.ref_count_cf()] {
            self.db
                .delete_cf(cf, hash.to_bytes())
                .map_err(|err| ExecutionError::RuntimeError(err.to_string()))?;
        }

        Ok(())
    }

    fn module_cf(&self) -> &ColumnFamily {
        self.db.cf_handle(MODULE_CF).expect(CF_ERROR)
    }

    fn gas_costs_cf(&self) -> &ColumnFamily {
        self.db.cf_handle(GAS_COSTS_CF).expect(CF_ERROR)
    }

    fn ref_count_cf(&self) -> &ColumnFamily {
        self.db.cf_handle(REF_COUNT_CF).expect(CF_ERROR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use massa_hash::Hash;
    use massa_sc_runtime::GasCosts;

    use std::path::PathBuf;

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
        HDCache::new(path)
    }

    #[test]
    fn test_insert_and_ref_count() {
        let cache = setup();
        let hash = Hash::compute_from(b"test_hash");
        let module = make_default_runtime_module();
        let init_cost = 100;

        cache
            .insert(hash, (module.clone(), Some(init_cost)))
            .expect("insert should succeed");

        let ref_count = cache.get_ref_count(hash).unwrap();
        assert_eq!(ref_count, 1u64);

        cache
            .set_ref_count(hash, 2u64)
            .expect("set_ref_count should succeed");
        let ref_count = cache.get_ref_count(hash).unwrap();
        assert_eq!(ref_count, 2u64);
    }

    #[test]
    fn test_insert_and_get() {
        let cache = setup();
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
    fn test_set_init_cost() {
        let cache = setup();
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

    #[test]
    fn test_get_and_increment() {
        let cache = setup();
        let hash = Hash::compute_from(b"test_hash");
        let module = make_default_runtime_module();
        let init_cost = 100;
        let limit = 1;

        if let Err(_) = cache.insert(hash, (module.clone(), Some(init_cost))) {
            assert!(false);
        };

        let (cached_module, cached_init_cost) = cache
            .get_and_increment(hash, limit, GasCosts::default())
            .unwrap();
        assert_eq!(
            cached_module.serialize().unwrap(),
            module.serialize().unwrap()
        );
        assert_eq!(cached_init_cost.unwrap(), init_cost);

        let (_, cached_init_cost) = cache.get(hash, limit, GasCosts::default()).unwrap();
        assert_eq!(cached_init_cost.unwrap(), init_cost + 1);
    }

    #[test]
    fn test_remove() {
        let cache = setup();
        let hash = Hash::compute_from(b"test_hash");
        let module = make_default_runtime_module();
        let init_cost = 100;
        let limit = 1;

        if let Err(_) = cache.insert(hash, (module.clone(), Some(init_cost))) {
            assert!(false);
        };

        assert!(cache.get(hash, limit, GasCosts::default()).is_some());

        cache.remove(hash).expect("remove should succeed");
        assert!(cache.get(hash, limit, GasCosts::default()).is_none());
    }

    // #[bench]
    // fn bench_insert() {
    // }
}
