use std::{collections::HashMap, str::FromStr, sync::Arc};

use massa_execution_exports::{
    ExecutionBlockMetadata, ExecutionChannels, ExecutionConfig, ExecutionController,
    ExecutionError, ExecutionManager,
};
use massa_final_state::{test_exports::get_sample_state, FinalStateController};
use massa_metrics::MassaMetrics;
use massa_models::{
    address::Address,
    amount::Amount,
    block_id::BlockId,
    config::MIP_STORE_STATS_BLOCK_CONSIDERED,
    datastore::Datastore,
    operation::{Operation, OperationSerializer, OperationType, SecureShareOperation},
    prehash::PreHashMap,
    secure_share::SecureShareContent,
    slot::Slot,
};
use massa_pos_exports::MockSelectorControllerWrapper;
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_test_framework::TestUniverse;
use massa_versioning::versioning::{MipStatsConfig, MipStore};
use massa_wallet::test_exports::create_test_wallet;
use num::rational::Ratio;
use parking_lot::RwLock;
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
    pub final_state: Arc<RwLock<dyn FinalStateController>>,
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
            final_state.clone(),
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
            final_state,
            module_controller,
            module_manager,
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
