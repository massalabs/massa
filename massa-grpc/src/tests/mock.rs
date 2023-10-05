use std::collections::{BTreeMap, HashMap};

use crate::config::{GrpcConfig, ServiceName};
use crate::server::MassaPublicGrpc;
use massa_channel::MassaChannel;
use massa_consensus_exports::test_exports::MockConsensusControllerImpl;
use massa_consensus_exports::ConsensusChannels;
use massa_execution_exports::ExecutionChannels;
use massa_models::address::Address;
use massa_models::block_id::BlockId;
use massa_models::slot::Slot;
use massa_models::stats::ExecutionStats;
use massa_models::{
    config::{
        ENDORSEMENT_COUNT, GENESIS_TIMESTAMP, MAX_DATASTORE_VALUE_LENGTH,
        MAX_DENUNCIATIONS_PER_BLOCK_HEADER, MAX_ENDORSEMENTS_PER_MESSAGE, MAX_FUNCTION_NAME_LENGTH,
        MAX_OPERATIONS_PER_BLOCK, MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        MAX_OPERATION_DATASTORE_KEY_LENGTH, MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        MAX_PARAMETERS_SIZE, MIP_STORE_STATS_BLOCK_CONSIDERED, PERIODS_PER_CYCLE, T0, THREAD_COUNT,
        VERSION,
    },
    node::NodeId,
};
use massa_pool_exports::test_exports::MockPoolController;
use massa_pool_exports::PoolChannels;
use massa_pos_exports::test_exports::MockSelectorController;
use massa_pos_exports::Selection;
use massa_protocol_exports::{MockProtocolController, ProtocolConfig};
use massa_signature::KeyPair;
use massa_versioning::keypair_factory::KeyPairFactory;
use massa_versioning::versioning::{MipStatsConfig, MipStore};
// use massa_wallet::test_exports::create_test_wallet;
use num::rational::Ratio;
use std::path::PathBuf;

use massa_execution_exports::{
    ExecutionAddressInfo, ExecutionBlockMetadata, ExecutionController, ExecutionError,
    ExecutionQueryRequest, ExecutionQueryResponse, ReadOnlyExecutionOutput,
    ReadOnlyExecutionRequest,
};
use massa_models::{
    amount::Amount,
    denunciation::{Denunciation, DenunciationIndex, DenunciationPrecursor},
    endorsement::EndorsementId,
    execution::EventFilter,
    operation::OperationId,
    output_event::SCOutputEvent,
    prehash::{PreHashMap, PreHashSet},
};
use massa_pool_exports::PoolController;
use massa_pos_exports::{PosResult, SelectorController};
use massa_storage::Storage;

#[cfg(any(test, feature = "testing"))]
mockall::mock! {

    pub ExecutionCtrl {}
    impl Clone for ExecutionCtrl {
        fn clone(&self) -> Self;
    }

    impl ExecutionController for ExecutionCtrl {

    fn update_blockclique_status(
        &self,
        finalized_blocks: HashMap<Slot, BlockId>,
        new_blockclique: Option<HashMap<Slot, BlockId>>,
        block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata>,
    );

    /// Atomically query the execution state with multiple requests
    fn query_state(&self, req: ExecutionQueryRequest) -> ExecutionQueryResponse;

    fn get_filtered_sc_output_event(&self, filter: EventFilter) -> Vec<SCOutputEvent>;

    fn get_final_and_candidate_balance(
        &self,
        addresses: &[Address],
    ) -> Vec<(Option<Amount>, Option<Amount>)>;

    fn get_ops_exec_status(&self, batch: &[OperationId]) -> Vec<(Option<bool>, Option<bool>)>;

    #[allow(clippy::type_complexity)]
    fn get_final_and_active_data_entry(
        &self,
        input: Vec<(Address, Vec<u8>)>,
    ) -> Vec<(Option<Vec<u8>>, Option<Vec<u8>>)>;

    fn get_cycle_active_rolls(&self, cycle: u64) -> BTreeMap<Address, u64>;

    fn execute_readonly_request(
        &self,
        req: ReadOnlyExecutionRequest,
    ) -> Result<ReadOnlyExecutionOutput, ExecutionError>;

    fn get_denunciation_execution_status(
        &self,
        denunciation_index: &DenunciationIndex,
    ) -> (bool, bool);

    fn get_addresses_infos(&self, addresses: &[Address]) -> Vec<ExecutionAddressInfo>;

    fn get_stats(&self) -> ExecutionStats;

    fn clone_box(&self) -> Box<dyn ExecutionController>;
    }



}

#[cfg(any(test, feature = "testing"))]
mockall::mock! {
    pub PoolCtrl{}

    impl PoolController for PoolCtrl {
        /// Asynchronously add operations to pool. Simply print a warning on failure.
        fn add_operations(&mut self, ops: Storage);

        /// Asynchronously add endorsements to pool. Simply print a warning on failure.
        fn add_endorsements(&mut self, endorsements: Storage);

        /// Add denunciation precursor to pool
        fn add_denunciation_precursor(&self, denunciation_precursor: DenunciationPrecursor);

        /// Asynchronously notify of new consensus final periods. Simply print a warning on failure.
        fn notify_final_cs_periods(&mut self, final_cs_periods: &[u64]);

        /// Get operations for block creation.
        fn get_block_operations(&self, slot: &Slot) -> (Vec<OperationId>, Storage);

        /// Get endorsements for a block.
        fn get_block_endorsements(
            &self,
            target_block: &BlockId,
            slot: &Slot,
        ) -> (Vec<Option<EndorsementId>>, Storage);

        /// Get denunciations for a block header.
        fn get_block_denunciations(&self, target_slot: &Slot) -> Vec<Denunciation>;

        /// Get the number of endorsements in the pool
        fn get_endorsement_count(&self) -> usize;

        /// Get the number of operations in the pool
        fn get_operation_count(&self) -> usize;

        /// Check if the pool contains a list of endorsements. Returns one boolean per item.
        fn contains_endorsements(&self, endorsements: &[EndorsementId]) -> Vec<bool>;

        /// Check if the pool contains a list of operations. Returns one boolean per item.
        fn contains_operations(&self, operations: &[OperationId]) -> Vec<bool>;

        /// Check if the pool contains a denunciation. Returns a boolean
        fn contains_denunciation(&self, denunciation: &Denunciation) -> bool;

        /// Get the number of denunciations in the pool
        fn get_denunciation_count(&self) -> usize;

        /// Returns a boxed clone of self.
        /// Useful to allow cloning `Box<dyn PoolController>`.
        fn clone_box(&self) -> Box<dyn PoolController>;

        /// Get final cs periods (updated regularly from consensus)
        fn get_final_cs_periods(&self) -> &Vec<u64>;
    }
}

#[cfg(any(test, feature = "testing"))]
mockall::mock! {
    pub SelectorCtrl {}

    impl SelectorController for SelectorCtrl {
        fn wait_for_draws(&self, cycle: u64) -> PosResult<u64>;

        fn feed_cycle(
            &self,
            cycle: u64,
            lookback_rolls: BTreeMap<Address, u64>,
            lookback_seed: massa_hash::Hash,
        ) -> PosResult<()>;

        fn get_selection(&self, slot: Slot) -> PosResult<Selection>;

        fn get_producer(&self, slot: Slot) -> PosResult<Address>;

        #[allow(clippy::needless_lifetimes)] // lifetime elision conflicts with Mockall
        fn get_available_selections_in_range<'a>(
            &self,
            slot_range: std::ops::RangeInclusive<Slot>,
            restrict_to_addresses: Option<&'a PreHashSet<Address>>,
        ) -> PosResult<BTreeMap<Slot, Selection>>;

        fn clone_box(&self) -> Box<dyn SelectorController>;

        #[cfg(feature = "testing")]
        fn get_entire_selection(&self) -> std::collections::VecDeque<(u64,HashMap<Slot,Selection>)> {
            unimplemented!("mock implementation only")
        }
    }

}

pub(crate) fn grpc_public_service() -> MassaPublicGrpc {
    let consensus_controller = MockConsensusControllerImpl::new();

    let shared_storage: massa_storage::Storage = massa_storage::Storage::create_root();
    let selector_ctrl = MockSelectorController::new_with_receiver();
    let pool_ctrl = MockPoolController::new_with_receiver();
    let (consensus_event_sender, _consensus_event_receiver) =
        MassaChannel::new("consensus_event".to_string(), Some(1024));

    let consensus_channels = ConsensusChannels {
        execution_controller: Box::new(MockExecutionCtrl::new()),
        selector_controller: selector_ctrl.0.clone(),
        pool_controller: pool_ctrl.0.clone(),
        protocol_controller: Box::new(MockProtocolController::new()),
        controller_event_tx: consensus_event_sender,
        block_sender: tokio::sync::broadcast::channel(100).0,
        block_header_sender: tokio::sync::broadcast::channel(100).0,
        filled_block_sender: tokio::sync::broadcast::channel(100).0,
    };
    let endorsement_sender = tokio::sync::broadcast::channel(2000).0;
    let operation_sender = tokio::sync::broadcast::channel(5000).0;
    let slot_execution_output_sender = tokio::sync::broadcast::channel(5000).0;
    let keypair = KeyPair::generate(0).unwrap();
    let grpc_config = GrpcConfig {
        name: ServiceName::Public,
        enabled: true,
        accept_http1: true,
        enable_cors: true,
        enable_health: true,
        enable_reflection: true,
        enable_tls: false,
        enable_mtls: false,
        generate_self_signed_certificates: false,
        subject_alt_names: vec![],
        bind: "[::]:8888".parse().unwrap(),
        // bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8888),
        accept_compressed: None,
        send_compressed: None,
        max_decoding_message_size: 4194304,
        max_encoding_message_size: 4194304,
        max_gas_per_block: u32::MAX as u64,
        concurrency_limit_per_connection: 5,
        timeout: Default::default(),
        initial_stream_window_size: None,
        initial_connection_window_size: None,
        max_concurrent_streams: None,
        max_arguments: 128,
        tcp_keepalive: None,
        tcp_nodelay: false,
        http2_keepalive_interval: None,
        http2_keepalive_timeout: None,
        http2_adaptive_window: None,
        max_frame_size: None,
        thread_count: THREAD_COUNT,
        max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
        endorsement_count: ENDORSEMENT_COUNT,
        max_endorsements_per_message: MAX_ENDORSEMENTS_PER_MESSAGE,
        max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
        max_op_datastore_entry_count: MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        max_datastore_entries_per_request: MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        max_op_datastore_key_length: MAX_OPERATION_DATASTORE_KEY_LENGTH,
        max_op_datastore_value_length: MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        max_function_name_length: MAX_FUNCTION_NAME_LENGTH,
        max_parameter_size: MAX_PARAMETERS_SIZE,
        max_operations_per_message: 2,
        genesis_timestamp: *GENESIS_TIMESTAMP,
        t0: T0,
        periods_per_cycle: PERIODS_PER_CYCLE,
        keypair: keypair.clone(),
        max_channel_size: 128,
        draw_lookahead_period_count: 10,
        last_start_period: 0,
        max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
        max_addresses_per_request: 50,
        max_slot_ranges_per_request: 50,
        max_block_ids_per_request: 50,
        max_endorsement_ids_per_request: 100,
        max_operation_ids_per_request: 250,
        max_filters_per_request: 32,
        server_certificate_path: PathBuf::default(),
        server_private_key_path: PathBuf::default(),
        certificate_authority_root_path: PathBuf::default(),
        client_certificate_authority_root_path: PathBuf::default(),
        client_certificate_path: PathBuf::default(),
        client_private_key_path: PathBuf::default(),
    };

    let mip_stats_config = MipStatsConfig {
        block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
        warn_announced_version_ratio: Ratio::new_raw(30, 100),
    };

    let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

    MassaPublicGrpc {
        consensus_controller: Box::new(consensus_controller),
        consensus_channels,
        execution_controller: Box::new(MockExecutionCtrl::new()),
        execution_channels: ExecutionChannels {
            slot_execution_output_sender,
        },
        pool_channels: PoolChannels {
            endorsement_sender,
            operation_sender,
            selector: selector_ctrl.0.clone(),
            execution_controller: Box::new(MockExecutionCtrl::new()),
        },
        pool_controller: pool_ctrl.0,
        protocol_controller: Box::new(MockProtocolController::new()),
        protocol_config: ProtocolConfig::default(),
        selector_controller: selector_ctrl.0,
        storage: shared_storage,
        grpc_config: grpc_config.clone(),
        version: *VERSION,
        node_id: NodeId::new(keypair.get_public_key()),
        keypair_factory: KeyPairFactory {
            mip_store: mip_store.clone(),
        },
    }
}
