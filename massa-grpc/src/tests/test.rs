// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::config::GrpcConfig;
use crate::server::MassaGrpc;
use massa_channel::MassaChannel;
use massa_consensus_exports::{test_exports::MockConsensusControllerImpl, ConsensusChannels};
use massa_execution_exports::{ExecutionChannels, MockExecutionController};
use massa_models::config::{
    ENDORSEMENT_COUNT, GENESIS_TIMESTAMP, MAX_DATASTORE_VALUE_LENGTH,
    MAX_DENUNCIATIONS_PER_BLOCK_HEADER, MAX_ENDORSEMENTS_PER_MESSAGE, MAX_FUNCTION_NAME_LENGTH,
    MAX_OPERATIONS_PER_BLOCK, MAX_OPERATIONS_PER_MESSAGE, MAX_OPERATION_DATASTORE_ENTRY_COUNT,
    MAX_OPERATION_DATASTORE_KEY_LENGTH, MAX_OPERATION_DATASTORE_VALUE_LENGTH, MAX_PARAMETERS_SIZE,
    MIP_STORE_STATS_BLOCK_CONSIDERED, MIP_STORE_STATS_COUNTERS_MAX, PERIODS_PER_CYCLE, T0,
    THREAD_COUNT, VERSION,
};
use massa_pool_exports::test_exports::MockPoolController;
use massa_pool_exports::PoolChannels;
use massa_pos_exports::test_exports::MockSelectorController;
use massa_proto::massa::api::v1::massa_service_client::MassaServiceClient;
use massa_protocol_exports::MockProtocolController;
use massa_versioning::versioning::{MipStatsConfig, MipStore};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
};

#[tokio::test]
async fn test_start_grpc_server() {
    let consensus_controller = MockConsensusControllerImpl::new();
    let shared_storage: massa_storage::Storage = massa_storage::Storage::create_root();
    let selector_ctrl = MockSelectorController::new_with_receiver();
    let pool_ctrl = MockPoolController::new_with_receiver();
    let (consensus_event_sender, _consensus_event_receiver) =
        MassaChannel::new("consensus_event".to_string(), Some(1024));

    let consensus_channels = ConsensusChannels {
        execution_controller: Box::new(MockExecutionController::new()),
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

    let grpc_config = GrpcConfig {
        enabled: true,
        accept_http1: true,
        enable_cors: true,
        enable_health: true,
        enable_reflection: true,
        enable_mtls: false,
        bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8888),
        accept_compressed: None,
        send_compressed: None,
        max_decoding_message_size: 4194304,
        max_encoding_message_size: 4194304,
        concurrency_limit_per_connection: 0,
        timeout: Default::default(),
        initial_stream_window_size: None,
        initial_connection_window_size: None,
        max_concurrent_streams: None,
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
        max_op_datastore_key_length: MAX_OPERATION_DATASTORE_KEY_LENGTH,
        max_op_datastore_value_length: MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        max_function_name_length: MAX_FUNCTION_NAME_LENGTH,
        max_parameter_size: MAX_PARAMETERS_SIZE,
        max_operations_per_message: MAX_OPERATIONS_PER_MESSAGE,
        genesis_timestamp: *GENESIS_TIMESTAMP,
        t0: T0,
        periods_per_cycle: PERIODS_PER_CYCLE,
        max_channel_size: 128,
        draw_lookahead_period_count: 10,
        last_start_period: 0,
        max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
        max_block_ids_per_request: 50,
        max_operation_ids_per_request: 250,
        server_certificate_path: PathBuf::default(),
        server_private_key_path: PathBuf::default(),
        client_certificate_authority_root_path: PathBuf::default(),
    };

    let mip_stats_config = MipStatsConfig {
        block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
        counters_max: MIP_STORE_STATS_COUNTERS_MAX,
    };

    let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

    let service = MassaGrpc {
        consensus_controller: Box::new(consensus_controller),
        consensus_channels,
        execution_controller: Box::new(MockExecutionController::new()),
        execution_channels: ExecutionChannels {
            slot_execution_output_sender,
        },
        pool_channels: PoolChannels {
            endorsement_sender,
            operation_sender,
            selector: selector_ctrl.0.clone(),
        },
        pool_command_sender: pool_ctrl.0,
        protocol_command_sender: Box::new(MockProtocolController::new()),
        selector_controller: selector_ctrl.0,
        storage: shared_storage,
        grpc_config: grpc_config.clone(),
        version: *VERSION,
        mip_store,
    };

    let stop_handle = service.serve(&grpc_config).await.unwrap();
    // std::thread::sleep(Duration::from_millis(100));

    // start grpc client and connect to the server
    let channel = tonic::transport::Channel::from_static("grpc://localhost:8888")
        .connect()
        .await
        .unwrap();

    let _res = MassaServiceClient::new(channel);
    stop_handle.stop();
}
