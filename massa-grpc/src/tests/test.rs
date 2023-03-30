// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::config::GrpcConfig;
use crate::service::MassaGrpcService;
use massa_consensus_exports::test_exports::MockConsensusController;
use massa_consensus_exports::ConsensusChannels;
use massa_execution_exports::test_exports::MockExecutionController;
use massa_models::config::{
    ENDORSEMENT_COUNT, GENESIS_TIMESTAMP, MAX_DATASTORE_VALUE_LENGTH, MAX_ENDORSEMENTS_PER_MESSAGE,
    MAX_FUNCTION_NAME_LENGTH, MAX_OPERATIONS_PER_BLOCK, MAX_OPERATIONS_PER_MESSAGE,
    MAX_OPERATION_DATASTORE_ENTRY_COUNT, MAX_OPERATION_DATASTORE_KEY_LENGTH,
    MAX_OPERATION_DATASTORE_VALUE_LENGTH, MAX_PARAMETERS_SIZE, PROTOCOL_CONTROLLER_CHANNEL_SIZE,
    T0, THREAD_COUNT, VERSION,
};
use massa_pool_exports::test_exports::MockPoolController;
use massa_pool_exports::PoolChannels;
use massa_pos_exports::test_exports::MockSelectorController;
use massa_proto::massa::api::v1::grpc_client::GrpcClient;
use massa_protocol_exports::{ProtocolCommand, ProtocolCommandSender};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tokio::sync::mpsc;

#[tokio::test]
async fn test_start_grpc_server() {
    let consensus_controller = MockConsensusController::new_with_receiver();
    let execution_ctrl = MockExecutionController::new_with_receiver();
    let shared_storage: massa_storage::Storage = massa_storage::Storage::create_root();
    let selector_ctrl = MockSelectorController::new_with_receiver();
    let pool_ctrl = MockPoolController::new_with_receiver();

    let (consensus_event_sender, _consensus_event_receiver) = crossbeam::channel::bounded(1024);

    let (protocol_command_sender, _protocol_command_receiver) =
        mpsc::channel::<ProtocolCommand>(PROTOCOL_CONTROLLER_CHANNEL_SIZE);

    let consensus_channels = ConsensusChannels {
        execution_controller: execution_ctrl.0.clone(),
        selector_controller: selector_ctrl.0.clone(),
        pool_command_sender: pool_ctrl.0.clone(),
        controller_event_tx: consensus_event_sender,
        protocol_command_sender: ProtocolCommandSender(protocol_command_sender.clone()),
        block_sender: tokio::sync::broadcast::channel(100).0,
        block_header_sender: tokio::sync::broadcast::channel(100).0,
        filled_block_sender: tokio::sync::broadcast::channel(100).0,
    };

    let operation_sender = tokio::sync::broadcast::channel(5000).0;

    let grpc_config = GrpcConfig {
        enabled: true,
        accept_http1: true,
        enable_reflection: true,
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
        max_channel_size: 128,
        draw_lookahead_period_count: 10,
    };

    let service = MassaGrpcService {
        consensus_controller: consensus_controller.0,
        consensus_channels,
        execution_controller: execution_ctrl.0,
        pool_channels: PoolChannels { operation_sender },
        pool_command_sender: pool_ctrl.0,
        protocol_command_sender: ProtocolCommandSender(protocol_command_sender.clone()),
        selector_controller: selector_ctrl.0,
        storage: shared_storage,
        grpc_config: grpc_config.clone(),
        version: *VERSION,
    };

    let stop_handle = service.serve(&grpc_config).await.unwrap();
    // std::thread::sleep(Duration::from_millis(100));

    // start grpc client and connect to the server
    let channel = tonic::transport::Channel::from_static("grpc://localhost:8888")
        .connect()
        .await
        .unwrap();

    let _res = GrpcClient::new(channel);
    stop_handle.stop();
}
