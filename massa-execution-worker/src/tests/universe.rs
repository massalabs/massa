use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::Seek,
    str::FromStr,
    sync::Arc,
};

use massa_db_exports::{DBBatch, MassaDBConfig, MassaDBController};
use massa_db_worker::MassaDB;
use massa_execution_exports::{
    ExecutionBlockMetadata, ExecutionChannels, ExecutionConfig, ExecutionController,
    ExecutionError, ExecutionManager,
};
use massa_final_state::{FinalState, FinalStateConfig};
use massa_ledger_exports::{LedgerConfig, LedgerController, LedgerEntry, LedgerError};
use massa_ledger_worker::FinalLedger;
use massa_metrics::MassaMetrics;
use massa_models::{
    address::Address,
    amount::Amount,
    block_id::BlockId,
    config::{
        ENDORSEMENT_COUNT, GENESIS_TIMESTAMP, MIP_STORE_STATS_BLOCK_CONSIDERED, T0, THREAD_COUNT,
    },
    datastore::Datastore,
    operation::{Operation, OperationSerializer, OperationType, SecureShareOperation},
    prehash::PreHashMap,
    secure_share::SecureShareContent,
    slot::Slot,
};
use massa_pos_exports::{MockSelectorControllerWrapper, SelectorController};
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_test_framework::TestUniverse;
use massa_versioning::versioning::{MipStatsConfig, MipStore};
use massa_wallet::test_exports::create_test_wallet;
use num::rational::Ratio;
use parking_lot::RwLock;
use tempfile::{NamedTempFile, TempDir};
use tokio::sync::broadcast;

use crate::start_execution_worker;

pub struct ExecutionForeignControllers {
    pub selector_controller: Box<MockSelectorControllerWrapper>,
}

impl ExecutionForeignControllers {
    pub fn new_with_mocks() -> Self {
        Self {
            selector_controller: Box::new(MockSelectorControllerWrapper::new()),
        }
    }
}

pub struct ExecutionTestUniverse {
    pub module_controller: Box<dyn ExecutionController>,
    pub storage: Storage,
    module_manager: Box<dyn ExecutionManager>,
}

impl TestUniverse for ExecutionTestUniverse {
    type ForeignControllers = ExecutionForeignControllers;
    type Config = ExecutionConfig;

    fn new(controllers: Self::ForeignControllers, config: Self::Config) -> Self {
        let storage = Storage::create_root();
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            warn_announced_version_ratio: Ratio::new_raw(30, 100),
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();
        let (final_state, _, _) = get_sample_state(
            config.last_start_period,
            controllers.selector_controller.clone(),
            mip_store.clone(),
        )
        .unwrap();
        let (tx, _) = broadcast::channel(16);
        let (module_manager, module_controller) = start_execution_worker(
            config.clone(),
            final_state,
            controllers.selector_controller,
            mip_store,
            ExecutionChannels {
                slot_execution_output_sender: tx,
            },
            Arc::new(RwLock::new(create_test_wallet(Some(PreHashMap::default())))),
            MassaMetrics::new(
                false,
                "0.0.0.0:9898".parse().unwrap(),
                32,
                std::time::Duration::from_secs(5),
            )
            .0,
        );
        init_execution_worker(&config, &storage, module_controller.clone());
        let universe = Self {
            storage,
            module_controller,
            module_manager,
        };
        universe.initialize();
        universe
    }
}

impl ExecutionTestUniverse {
    /// Create an operation for the given sender with `data` as bytecode.
    pub fn create_execute_sc_operation(
        sender_keypair: &KeyPair,
        data: &[u8],
        datastore: Datastore,
    ) -> Result<SecureShareOperation, ExecutionError> {
        let op = OperationType::ExecuteSC {
            data: data.to_vec(),
            max_gas: 100_000_000,
            max_coins: Amount::from_str("5000000").unwrap(),
            datastore,
        };
        let op = Operation::new_verifiable(
            Operation {
                fee: Amount::const_init(10, 0),
                expire_period: 10,
                op,
            },
            OperationSerializer::new(),
            sender_keypair,
        )?;
        Ok(op)
    }

    /// Create an operation for the given sender with `data` as bytecode.
    pub fn create_call_sc_operation(
        sender_keypair: &KeyPair,
        max_gas: u64,
        fee: Amount,
        coins: Amount,
        target_addr: Address,
        target_func: String,
        param: Vec<u8>,
    ) -> Result<SecureShareOperation, ExecutionError> {
        let op = OperationType::CallSC {
            max_gas,
            target_addr,
            coins,
            target_func,
            param,
        };
        let op = Operation::new_verifiable(
            Operation {
                fee,
                expire_period: 10,
                op,
            },
            OperationSerializer::new(),
            sender_keypair,
        )?;
        Ok(op)
    }

    pub fn stop(&mut self) {
        self.module_manager.stop();
    }
}

// Maybe move it to the global trait TestUniverse if others modules need it
fn get_sample_state(
    last_start_period: u64,
    selector_controller: Box<dyn SelectorController>,
    mip_store: MipStore,
) -> Result<(Arc<RwLock<FinalState>>, NamedTempFile, TempDir), LedgerError> {
    let (rolls_file, ledger) = get_initials();
    let (ledger_config, tempfile, tempdir) = LedgerConfig::sample(&ledger);
    let db_config = MassaDBConfig {
        path: tempdir.path().to_path_buf(),
        max_history_length: 10,
        max_final_state_elements_size: 100_000,
        max_versioning_elements_size: 100_000,
        thread_count: THREAD_COUNT,
    };
    let db = Arc::new(RwLock::new(
        Box::new(MassaDB::new(db_config)) as Box<(dyn MassaDBController + 'static)>
    ));

    let mut ledger = FinalLedger::new(ledger_config.clone(), db.clone());
    ledger.load_initial_ledger().unwrap();
    let default_config = FinalStateConfig::default();
    let cfg = FinalStateConfig {
        ledger_config,
        async_pool_config: default_config.async_pool_config,
        pos_config: default_config.pos_config,
        executed_ops_config: default_config.executed_ops_config,
        executed_denunciations_config: default_config.executed_denunciations_config,
        final_history_length: 128,
        thread_count: THREAD_COUNT,
        initial_rolls_path: rolls_file.path().to_path_buf(),
        endorsement_count: ENDORSEMENT_COUNT,
        max_executed_denunciations_length: 1000,
        initial_seed_string: "".to_string(),
        periods_per_cycle: 10,
        max_denunciations_per_block_header: 0,
        t0: T0,
        genesis_timestamp: *GENESIS_TIMESTAMP,
    };

    let mut final_state = if last_start_period > 0 {
        FinalState::new_derived_from_snapshot(
            db.clone(),
            cfg,
            Box::new(ledger),
            selector_controller,
            mip_store,
            last_start_period,
        )
        .unwrap()
    } else {
        FinalState::new(
            db.clone(),
            cfg,
            Box::new(ledger),
            selector_controller,
            mip_store,
            true,
        )
        .unwrap()
    };

    let mut batch: BTreeMap<Vec<u8>, Option<Vec<u8>>> = DBBatch::new();
    final_state.pos_state.create_initial_cycle(&mut batch);
    final_state.init_execution_trail_hash_to_batch(&mut batch);
    final_state
        .db
        .write()
        .write_batch(batch, Default::default(), None);
    final_state.compute_initial_draws().unwrap();
    Ok((Arc::new(RwLock::new(final_state)), tempfile, tempdir))
}

fn get_initials() -> (NamedTempFile, HashMap<Address, LedgerEntry>) {
    let file = NamedTempFile::new().unwrap();
    let mut rolls: BTreeMap<Address, u64> = BTreeMap::new();
    let mut ledger: HashMap<Address, LedgerEntry> = HashMap::new();

    let raw_keypairs = [
        "S18r2i8oJJyhF7Kprx98zwxAc3W4szf7RKuVMX6JydZz8zSxHeC", // thread 0
        "S1FpYC4ugG9ivZZbLVrTwWtF9diSRiAwwrVX5Gx1ANSRLfouUjq", // thread 1
        "S1LgXhWLEgAgCX3nm6y8PVPzpybmsYpi6yg6ZySwu5Z4ERnD7Bu", // thread 2
    ];

    for s in raw_keypairs {
        let keypair = KeyPair::from_str(s).unwrap();
        let addr = Address::from_public_key(&keypair.get_public_key());
        rolls.insert(addr, 100);
        ledger.insert(
            addr,
            LedgerEntry {
                balance: Amount::from_str("300_000").unwrap(),
                ..Default::default()
            },
        );
    }

    // write file
    serde_json::to_writer_pretty::<&File, BTreeMap<Address, u64>>(file.as_file(), &rolls)
        .expect("unable to write ledger file");
    file.as_file()
        .seek(std::io::SeekFrom::Start(0))
        .expect("could not seek file");

    (file, ledger)
}

/// Feeds the execution worker with genesis blocks to start it
fn init_execution_worker(
    config: &ExecutionConfig,
    storage: &Storage,
    execution_controller: Box<dyn ExecutionController>,
) {
    let genesis_keypair = KeyPair::generate(0).unwrap();
    let genesis_addr = Address::from_public_key(&genesis_keypair.get_public_key());
    let mut finalized_blocks: HashMap<Slot, BlockId> = HashMap::new();
    let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = PreHashMap::default();
    for thread in 0..config.thread_count {
        let slot = Slot::new(0, thread);
        let final_block = ExecutionTestUniverse::create_block(&genesis_keypair, slot, None, None);
        finalized_blocks.insert(slot, final_block.id);
        let mut final_block_storage = storage.clone_without_refs();
        final_block_storage.store_block(final_block.clone());
        block_metadata.insert(
            final_block.id,
            ExecutionBlockMetadata {
                same_thread_parent_creator: Some(genesis_addr),
                storage: Some(final_block_storage),
            },
        );
    }
    execution_controller.update_blockclique_status(
        finalized_blocks,
        Some(Default::default()),
        block_metadata,
    );
}
