use std::{
    collections::{BTreeMap, HashMap},
    str::FromStr,
    sync::Arc,
};

#[cfg(all(feature = "file_storage_backend", not(feature = "db_storage_backend")))]
use crate::storage_backend::FileStorageBackend;
#[cfg(feature = "db_storage_backend")]
use crate::storage_backend::RocksDBStorageBackend;
use cfg_if::cfg_if;
use massa_db_exports::{MassaDBConfig, MassaDBController, ShareableMassaDBController};
use massa_db_worker::MassaDB;
use massa_execution_exports::{
    ExecutionBlockMetadata, ExecutionChannels, ExecutionConfig, ExecutionController,
    ExecutionError, ExecutionManager, SlotExecutionOutput,
};
use massa_final_state::{FinalStateController, MockFinalStateController};
use massa_ledger_exports::MockLedgerControllerWrapper;
use massa_metrics::MassaMetrics;
use massa_models::config::CHAINID;
use massa_models::{
    address::Address,
    amount::Amount,
    block::SecureShareBlock,
    block_id::BlockId,
    config::{MIP_STORE_STATS_BLOCK_CONSIDERED, THREAD_COUNT},
    datastore::Datastore,
    execution::EventFilter,
    operation::{Operation, OperationSerializer, OperationType, SecureShareOperation},
    prehash::PreHashMap,
    secure_share::SecureShareContent,
    slot::Slot,
};
use massa_pos_exports::MockSelectorControllerWrapper;
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_test_framework::TestUniverse;
use massa_versioning::{
    mips::get_mip_list,
    versioning::{MipStatsConfig, MipStore},
};
use massa_wallet::test_exports::create_test_wallet;
use num::rational::Ratio;
use parking_lot::RwLock;
use tempfile::TempDir;
use tokio::sync::broadcast;

use crate::start_execution_worker;

#[cfg(feature = "execution-trace")]
use massa_execution_exports::types_trace_info::SlotAbiCallStack;

pub struct ExecutionForeignControllers {
    pub selector_controller: Box<MockSelectorControllerWrapper>,
    pub final_state: Arc<RwLock<MockFinalStateController>>,
    pub ledger_controller: MockLedgerControllerWrapper,
    pub db: ShareableMassaDBController,
}

impl ExecutionForeignControllers {
    pub fn new_with_mocks() -> Self {
        let disk_ledger = TempDir::new().expect("cannot create temp directory");
        let db_config = MassaDBConfig {
            path: disk_ledger.path().to_path_buf(),
            max_history_length: 10,
            max_final_state_elements_size: 100_000,
            max_versioning_elements_size: 100_000,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 10,
        };

        let db = Arc::new(RwLock::new(
            Box::new(MassaDB::new(db_config)) as Box<(dyn MassaDBController + 'static)>
        ));
        Self {
            selector_controller: Box::new(MockSelectorControllerWrapper::new()),
            ledger_controller: MockLedgerControllerWrapper::new(),
            final_state: Arc::new(RwLock::new(MockFinalStateController::new())),
            db,
        }
    }
}

pub struct ExecutionTestUniverse {
    pub module_controller: Box<dyn ExecutionController>,
    pub storage: Storage,
    pub final_state: Arc<RwLock<dyn FinalStateController>>,
    module_manager: Box<dyn ExecutionManager>,
    pub broadcast_channel_receiver: Option<tokio::sync::broadcast::Receiver<SlotExecutionOutput>>,
    #[cfg(feature = "execution-trace")]
    pub broadcast_traces_channel_receiver:
        Option<tokio::sync::broadcast::Receiver<(SlotAbiCallStack, bool)>>,
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
        let mip_list: [(
            massa_versioning::versioning::MipInfo,
            massa_versioning::versioning::MipState,
        ); 1] = get_mip_list();
        let mip_store =
            MipStore::try_from((mip_list, mip_stats_config)).expect("mip store creation failed");
        let (tx, rx) = broadcast::channel(16);
        #[cfg(feature = "execution-trace")]
        let (tx_traces, rx_traces) = broadcast::channel(16);
        let exec_channels = ExecutionChannels {
            slot_execution_output_sender: tx,
            #[cfg(feature = "execution-trace")]
            slot_execution_traces_sender: tx_traces,
        };

        cfg_if! {
            if #[cfg(all(feature = "dump-block", feature = "db_storage_backend"))] {
                let block_storage_backend = Arc::new(RwLock::new(RocksDBStorageBackend::new(
                    config.block_dump_folder_path.clone(),
                    10
                )));
            } else if #[cfg(all(feature = "dump-block", feature = "file_storage_backend"))] {
                let block_storage_backend = Arc::new(RwLock::new(FileStorageBackend::new(
                    config.block_dump_folder_path.clone(),
                    10
                )));
            } else if #[cfg(feature = "dump-block")] {
                compile_error!("feature dump-block require either db_storage_backend or file_storage_backend");
            }
        }

        let (module_manager, module_controller) = start_execution_worker(
            config.clone(),
            controllers.final_state.clone(),
            controllers.selector_controller,
            mip_store,
            exec_channels,
            Arc::new(RwLock::new(create_test_wallet(Some(PreHashMap::default())))),
            MassaMetrics::new(
                false,
                "0.0.0.0:9898".parse().unwrap(),
                32,
                std::time::Duration::from_secs(5),
            )
            .0,
            #[cfg(feature = "dump-block")]
            block_storage_backend.clone(),
        );

        init_execution_worker(&config, &storage, module_controller.clone());
        let universe = Self {
            storage,
            final_state: controllers.final_state,
            module_controller,
            module_manager,
            broadcast_channel_receiver: Some(rx),
            #[cfg(feature = "execution-trace")]
            broadcast_traces_channel_receiver: Some(rx_traces),
        };
        universe.initialize();
        universe
    }
}

impl Drop for ExecutionTestUniverse {
    fn drop(&mut self) {
        self.module_manager.stop();
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
            // MAX_GAS MUST BE AT LEAST 314_000_000 (SP COMPIL)
            // here we use 1.5B as most of the tests perform a SC creation:
            // 314_000_000 (SP COMPIL) + 745_000_000 (CL COMPIL) + margin
            max_gas: 1_500_000_000,
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
            *CHAINID,
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
            *CHAINID,
        )?;
        Ok(op)
    }

    pub fn deploy_bytecode_block(
        &mut self,
        keypair: &KeyPair,
        slot: Slot,
        bytes_file_sc_deployer: &[u8],
        bytes_file_sc_deployed: &[u8],
    ) {
        // load bytecodes
        // you can check the source code of the following wasm file in massa-unit-tests-src
        let mut datastore = BTreeMap::new();
        datastore.insert(b"smart-contract".to_vec(), bytes_file_sc_deployed.to_vec());

        // create the block containing the smart contract execution operation
        let operation = ExecutionTestUniverse::create_execute_sc_operation(
            keypair,
            bytes_file_sc_deployer,
            datastore,
        )
        .unwrap();
        self.storage.store_operations(vec![operation.clone()]);
        let block =
            ExecutionTestUniverse::create_block(keypair, slot, vec![operation], vec![], vec![]);

        // set our block as a final block so the message is sent
        self.send_and_finalize(keypair, block);
    }

    pub fn send_and_finalize(&mut self, keypair: &KeyPair, block: SecureShareBlock) {
        // store the block in storage
        self.storage.store_block(block.clone());
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
        block_metadata.insert(
            block.id,
            ExecutionBlockMetadata {
                same_thread_parent_creator: Some(Address::from_public_key(
                    &keypair.get_public_key(),
                )),
                storage: Some(self.storage.clone()),
            },
        );
        self.module_controller.update_blockclique_status(
            finalized_blocks.clone(),
            Default::default(),
            block_metadata.clone(),
        );
    }

    pub fn get_address_sc_deployed(&self, slot: Slot) -> String {
        let events = self
            .module_controller
            .get_filtered_sc_output_event(EventFilter {
                start: Some(slot),
                ..Default::default()
            });
        // match the events
        assert!(!events.is_empty(), "One event was expected");
        events[0].clone().data
    }

    pub fn call_sc_block(
        &mut self,
        keypair: &KeyPair,
        slot: Slot,
        operation: SecureShareOperation,
    ) {
        // Init new storage for this block
        self.storage.store_operations(vec![operation.clone()]);
        let block =
            ExecutionTestUniverse::create_block(keypair, slot, vec![operation], vec![], vec![]);
        // store the block in storage
        self.storage.store_block(block.clone());
        // set our block as a final block so the message is sent
        let mut finalized_blocks: HashMap<Slot, BlockId> = Default::default();
        finalized_blocks.insert(block.content.header.content.slot, block.id);
        let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> = Default::default();
        block_metadata.insert(
            block.id,
            ExecutionBlockMetadata {
                same_thread_parent_creator: Some(Address::from_public_key(
                    &keypair.get_public_key(),
                )),
                storage: Some(self.storage.clone()),
            },
        );
        self.module_controller.update_blockclique_status(
            finalized_blocks,
            Default::default(),
            block_metadata.clone(),
        );
    }
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
        let final_block =
            ExecutionTestUniverse::create_block(&genesis_keypair, slot, vec![], vec![], vec![]);
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
