// Copyright (c) 2023 MASSA LABS <info@massa.net>
use std::net::SocketAddr;

use crate::config::{GrpcConfig, ServiceName};
use crate::server::MassaPublicGrpc;
use massa_consensus_exports::{ConsensusBroadcasts, MockConsensusController};
use massa_execution_exports::{ExecutionChannels, MockExecutionController};
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
use massa_pool_exports::{MockPoolController, PoolBroadcasts};
use massa_pos_exports::MockSelectorController;
use massa_protocol_exports::{MockProtocolController, ProtocolConfig};
use massa_signature::KeyPair;
use massa_versioning::keypair_factory::KeyPairFactory;
use massa_versioning::versioning::{MipStatsConfig, MipStore};
// use massa_wallet::test_exports::create_test_wallet;
use num::rational::Ratio;
use std::path::PathBuf;

/// generate a grpc public service
/// # Arguments
/// * `addr` - the address to bind to
/// # Returns
/// * `MassaPublicGrpc` - the grpc public service
pub(crate) fn grpc_public_service(addr: &SocketAddr) -> MassaPublicGrpc {
    let consensus_ctrl = Box::new(MockConsensusController::new());
    let shared_storage: massa_storage::Storage = massa_storage::Storage::create_root();
    let selector_ctrl = Box::new(MockSelectorController::new());
    let pool_ctrl = Box::new(MockPoolController::new());
    let execution_ctrl = Box::new(MockExecutionController::new());
    let protocol_ctrl = Box::new(MockProtocolController::new());

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
        // bind: "[::]:8888".parse().unwrap(),
        bind: addr.clone(),
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
        max_query_items_per_request: 50,
    };

    let mip_stats_config = MipStatsConfig {
        block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
        warn_announced_version_ratio: Ratio::new_raw(30, 100),
    };

    let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

    MassaPublicGrpc {
        consensus_broadcasts: ConsensusBroadcasts {
            block_sender: tokio::sync::broadcast::channel(100).0,
            block_header_sender: tokio::sync::broadcast::channel(100).0,
            filled_block_sender: tokio::sync::broadcast::channel(100).0,
        },
        consensus_controller: consensus_ctrl,
        execution_controller: execution_ctrl,
        execution_channels: ExecutionChannels {
            slot_execution_output_sender,
        },
        pool_broadcasts: PoolBroadcasts {
            endorsement_sender,
            operation_sender,
        },
        pool_controller: pool_ctrl,
        protocol_controller: protocol_ctrl,
        protocol_config: ProtocolConfig::default(),
        selector_controller: selector_ctrl,
        storage: shared_storage,
        grpc_config: grpc_config.clone(),
        version: *VERSION,
        node_id: NodeId::new(keypair.get_public_key()),
        keypair_factory: KeyPairFactory {
            mip_store: mip_store.clone(),
        },
    }
}
