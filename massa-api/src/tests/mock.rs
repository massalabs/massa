//! Copyright (c) 2023 MASSA LABS <info@massa.net>
//!
//!

use std::{collections::HashMap, net::SocketAddr};

use massa_api_exports::config::APIConfig;
use massa_consensus_exports::{ConsensusBroadcasts, MockConsensusController};
use massa_execution_exports::{GasCosts, MockExecutionController};
use massa_models::amount::Amount;
use massa_models::config::CHAINID;
use massa_models::{
    config::{
        BASE_OPERATION_GAS_COST, ENDORSEMENT_COUNT, GENESIS_TIMESTAMP, MAX_DATASTORE_VALUE_LENGTH,
        MAX_FUNCTION_NAME_LENGTH, MAX_GAS_PER_BLOCK, MAX_MESSAGE_SIZE,
        MAX_OPERATION_DATASTORE_ENTRY_COUNT, MAX_OPERATION_DATASTORE_KEY_LENGTH,
        MAX_OPERATION_DATASTORE_VALUE_LENGTH, MAX_PARAMETERS_SIZE,
        MIP_STORE_STATS_BLOCK_CONSIDERED, PERIODS_PER_CYCLE, T0, THREAD_COUNT, VERSION,
    },
    node::NodeId,
};
use massa_pool_exports::{MockPoolController, PoolBroadcasts};
use massa_pos_exports::MockSelectorController;
use massa_protocol_exports::{MockProtocolController, PeerCategoryInfo, ProtocolConfig};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use massa_versioning::versioning::{MipStatsConfig, MipStore};
use num::rational::Ratio;
use tempfile::NamedTempFile;
use tokio::sync::broadcast;

use crate::{ApiV2, Public, API};

pub(crate) fn get_apiv2_server(addr: &SocketAddr) -> (API<ApiV2>, APIConfig) {
    let keypair = KeyPair::generate(0).unwrap();
    let api_config: APIConfig = APIConfig {
        bind_private: "[::]:0".parse().unwrap(),
        bind_public: "[::]:0".parse().unwrap(),
        bind_api: *addr,
        draw_lookahead_period_count: 10,
        max_arguments: 128,
        openrpc_spec_path: "base_config/openrpc.json".parse().unwrap(),
        bootstrap_whitelist_path: "base_config/bootstrap_whitelist.json".parse().unwrap(),
        bootstrap_blacklist_path: "base_config/bootstrap_blacklist.json".parse().unwrap(),
        max_request_body_size: 52428800,
        max_response_body_size: 52428800,
        max_connections: 100,
        max_subscriptions_per_connection: 1024,
        max_log_length: 4096,
        allow_hosts: vec![],
        batch_request_limit: 16,
        ping_interval: MassaTime::from_millis(60000),
        enable_http: true,
        enable_ws: true,
        max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
        max_op_datastore_entry_count: MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        max_op_datastore_key_length: MAX_OPERATION_DATASTORE_KEY_LENGTH,
        max_op_datastore_value_length: MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        max_gas_per_block: MAX_GAS_PER_BLOCK,
        base_operation_gas_cost: BASE_OPERATION_GAS_COST,
        sp_compilation_cost: GasCosts::default().sp_compilation_cost,
        max_function_name_length: MAX_FUNCTION_NAME_LENGTH,
        max_parameter_size: MAX_PARAMETERS_SIZE,
        thread_count: THREAD_COUNT,
        keypair: keypair.clone(),
        genesis_timestamp: *GENESIS_TIMESTAMP,
        t0: T0,
        periods_per_cycle: PERIODS_PER_CYCLE,
        last_start_period: 0,
        chain_id: *CHAINID,
        deferred_credits_delta: MassaTime::from_millis(24 * 3600 * 2),
        minimal_fees: Amount::zero(),
        deferred_calls_config: Default::default(),
    };

    // let shared_storage: massa_storage::Storage = massa_storage::Storage::create_root();

    // let mip_stats_config = MipStatsConfig {
    //     block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
    //     warn_announced_version_ratio: Ratio::new_raw(30, 100),
    // };

    // let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

    let consensus_ctrl = MockConsensusController::new();
    let exec_ctrl = MockExecutionController::new();

    let pool_broadcasts = PoolBroadcasts {
        endorsement_sender: broadcast::channel(100).0,
        operation_sender: broadcast::channel(100).0,
    };

    let consensus_broadcasts = ConsensusBroadcasts {
        block_header_sender: broadcast::channel(100).0,
        block_sender: broadcast::channel(100).0,
        filled_block_sender: broadcast::channel(100).0,
    };

    let api = API::<ApiV2>::new(
        Box::new(consensus_ctrl),
        consensus_broadcasts,
        Box::new(exec_ctrl),
        pool_broadcasts,
        api_config.clone(),
        *VERSION,
    );

    (api, api_config)
}

pub(crate) fn start_public_api(addr: SocketAddr) -> (API<Public>, APIConfig) {
    let keypair = KeyPair::generate(0).unwrap();
    let api_config: APIConfig = APIConfig {
        bind_private: "[::]:0".parse().unwrap(),
        bind_public: addr,
        bind_api: "[::]:0".parse().unwrap(),
        draw_lookahead_period_count: 10,
        max_arguments: 128,
        openrpc_spec_path: "base_config/openrpc.json".parse().unwrap(),
        bootstrap_whitelist_path: "base_config/bootstrap_whitelist.json".parse().unwrap(),
        bootstrap_blacklist_path: "base_config/bootstrap_blacklist.json".parse().unwrap(),
        max_request_body_size: 52428800,
        max_response_body_size: 52428800,
        max_connections: 100,
        max_subscriptions_per_connection: 1024,
        max_log_length: 4096,
        allow_hosts: vec![],
        batch_request_limit: 16,
        ping_interval: MassaTime::from_millis(60000),
        enable_http: true,
        enable_ws: true,
        max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
        max_op_datastore_entry_count: MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        max_op_datastore_key_length: MAX_OPERATION_DATASTORE_KEY_LENGTH,
        max_op_datastore_value_length: MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        max_gas_per_block: MAX_GAS_PER_BLOCK,
        base_operation_gas_cost: BASE_OPERATION_GAS_COST,
        sp_compilation_cost: GasCosts::default().sp_compilation_cost,
        max_function_name_length: MAX_FUNCTION_NAME_LENGTH,
        max_parameter_size: MAX_PARAMETERS_SIZE,
        thread_count: THREAD_COUNT,
        keypair: keypair.clone(),
        genesis_timestamp: *GENESIS_TIMESTAMP,
        t0: T0,
        periods_per_cycle: PERIODS_PER_CYCLE,
        last_start_period: 0,
        chain_id: *CHAINID,
        deferred_credits_delta: MassaTime::from_millis(24 * 3600 * 2),
        minimal_fees: Amount::zero(),
        deferred_calls_config: Default::default(),
    };

    let shared_storage: massa_storage::Storage = massa_storage::Storage::create_root();

    let mip_stats_config = MipStatsConfig {
        block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
        warn_announced_version_ratio: Ratio::new_raw(30, 100),
    };

    let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

    let consensus_ctrl = MockConsensusController::new();
    let exec_ctrl = MockExecutionController::new();
    let pool_ctrl = MockPoolController::new();
    let protocol_controller = MockProtocolController::new();
    let selector_ctrl = MockSelectorController::new();

    let api_public = API::<Public>::new(
        Box::new(consensus_ctrl),
        Box::new(exec_ctrl),
        api_config.clone(),
        Box::new(selector_ctrl),
        Box::new(pool_ctrl),
        Box::new(protocol_controller),
        ProtocolConfig {
            keypair_file: NamedTempFile::new()
                .expect("cannot create temp file")
                .path()
                .to_path_buf(),
            ask_block_timeout: MassaTime::from_millis(500),
            max_blocks_kept_for_propagation: 300,
            max_block_propagation_time: MassaTime::from_millis(40000),
            block_propagation_tick: MassaTime::from_millis(1000),
            max_known_blocks_size: 100,
            max_node_known_blocks_size: 100,
            max_node_wanted_blocks_size: 100,
            max_simultaneous_ask_blocks_per_node: 10,
            max_send_wait: MassaTime::from_millis(100),
            max_known_ops_size: 1000,
            max_node_known_ops_size: 1000,
            max_known_endorsements_size: 1000,
            max_node_known_endorsements_size: 1000,
            operation_batch_buffer_capacity: 1000,
            operation_announcement_buffer_capacity: 1000,
            max_operation_storage_time: MassaTime::from_millis(60000),
            operation_batch_proc_period: MassaTime::from_millis(200),
            asked_operations_buffer_capacity: 10000,
            operation_announcement_interval: MassaTime::from_millis(150),
            max_operations_per_message: 1024,
            max_operations_per_block: 5000,
            thread_count: 32,
            max_serialized_operations_size_per_block: 1024,
            controller_channel_size: 1024,
            event_channel_size: 1024,
            genesis_timestamp: MassaTime::now(),
            t0: MassaTime::from_millis(16000),
            max_ops_kept_for_propagation: 10000,
            max_operations_propagation_time: MassaTime::from_millis(30000),
            max_endorsements_propagation_time: MassaTime::from_millis(60000),
            initial_peers: NamedTempFile::new()
                .expect("cannot create temp file")
                .path()
                .to_path_buf(),
            listeners: HashMap::default(),
            thread_tester_count: 2,
            max_size_channel_commands_connectivity: 1000,
            max_size_channel_commands_retrieval_operations: 10000,
            max_size_channel_commands_propagation_operations: 10000,
            max_size_channel_commands_retrieval_blocks: 1000,
            max_size_channel_commands_propagation_blocks: 1000,
            max_size_channel_commands_propagation_endorsements: 5000,
            max_size_channel_commands_retrieval_endorsements: 5000,
            max_size_channel_network_to_block_handler: 1000,
            max_size_channel_network_to_endorsement_handler: 1000,
            max_size_channel_network_to_operation_handler: 10000,
            max_size_channel_network_to_peer_handler: 1000,
            max_size_channel_commands_peer_testers: 10000,
            max_size_channel_commands_peers: 300,
            max_message_size: MAX_MESSAGE_SIZE as usize,
            endorsement_count: ENDORSEMENT_COUNT,
            max_size_value_datastore: 1_000_000,
            max_size_function_name: u16::MAX,
            max_size_call_sc_parameter: 10_000_000,
            max_denunciations_in_block_header: 100,
            max_op_datastore_entry_count: 100000,
            max_op_datastore_key_length: u8::MAX,
            max_op_datastore_value_length: 1000000,
            max_endorsements_per_message: 1000,
            max_size_listeners_per_peer: 100,
            max_size_peers_announcement: 100,
            message_timeout: MassaTime::from_millis(10000),
            tester_timeout: MassaTime::from_millis(500),
            last_start_period: 0,
            read_write_limit_bytes_per_second: 1024 * 1000,
            timeout_connection: MassaTime::from_millis(1000),
            try_connection_timer: MassaTime::from_millis(5000),
            unban_everyone_timer: MassaTime::from_millis(3600000),
            routable_ip: None,
            max_in_connections: 10,
            debug: true,
            peers_categories: HashMap::default(),
            default_category_info: PeerCategoryInfo {
                allow_local_peers: true,
                max_in_connections: 10,
                target_out_connections: 10,
                max_in_connections_per_ip: 0,
            },
            version: *VERSION,
            try_connection_timer_same_peer: MassaTime::from_millis(1000),
            test_oldest_peer_cooldown: MassaTime::from_millis(720000),
            rate_limit: 1024 * 1024 * 2,
            chain_id: *CHAINID,
        },
        *VERSION,
        NodeId::new(keypair.get_public_key()),
        shared_storage,
        mip_store.clone(),
    );

    (api_public, api_config)
}
