// Copyright (c) 2023 MASSA LABS <info@massa.net>

use super::mock::MockExecutionCtrl;
use crate::config::{GrpcConfig, ServiceName};
use crate::server::MassaPublicGrpc;
use massa_channel::MassaChannel;
use massa_consensus_exports::test_exports::MockConsensusControllerImpl;
use massa_consensus_exports::ConsensusChannels;
use massa_execution_exports::EventStore;
use massa_execution_exports::{test_exports::MockExecutionController, ExecutionChannels};
use massa_models::address::Address;
use massa_models::block::BlockGraphStatus;
use massa_models::slot::Slot;
use massa_models::stats::ExecutionStats;
use massa_models::{
    config::{
        ENDORSEMENT_COUNT, GENESIS_TIMESTAMP, MAX_DATASTORE_VALUE_LENGTH,
        MAX_DENUNCIATIONS_PER_BLOCK_HEADER, MAX_ENDORSEMENTS_PER_MESSAGE, MAX_FUNCTION_NAME_LENGTH,
        MAX_OPERATIONS_PER_BLOCK, MAX_OPERATIONS_PER_MESSAGE, MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        MAX_OPERATION_DATASTORE_KEY_LENGTH, MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        MAX_PARAMETERS_SIZE, MIP_STORE_STATS_BLOCK_CONSIDERED, PERIODS_PER_CYCLE, T0, THREAD_COUNT,
        VERSION,
    },
    node::NodeId,
};
use massa_pool_exports::test_exports::MockPoolController;
use massa_pool_exports::PoolChannels;
use massa_pos_exports::test_exports::MockSelectorController;
use massa_proto_rs::massa::api::v1::get_datastore_entry_filter::Filter;
use massa_proto_rs::massa::api::v1::public_service_client::PublicServiceClient;
use massa_proto_rs::massa::api::v1::{
    ExecuteReadOnlyCallRequest, GetBlocksRequest, GetOperationsRequest, GetStatusRequest,
    GetTransactionsThroughputRequest,
};
use massa_proto_rs::massa::model::v1::read_only_execution_call::Target;
use massa_proto_rs::massa::model::v1::{BlockStatus, FunctionCall, ReadOnlyExecutionCall};
use massa_protocol_exports::test_exports::tools::{
    create_block, create_operation_with_expire_period,
};
use massa_protocol_exports::{MockProtocolController, ProtocolConfig};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use massa_versioning::{
    keypair_factory::KeyPairFactory,
    versioning::{MipStatsConfig, MipStore},
};
// use massa_wallet::test_exports::create_test_wallet;
use num::rational::Ratio;
use std::str::FromStr;
use std::time::Duration;
use std::{net::SocketAddr, path::PathBuf};

const GRPC_SERVER_URL: &str = "grpc://localhost:8888";

fn grpc_public_service() -> MassaPublicGrpc {
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
        max_operations_per_message: MAX_OPERATIONS_PER_MESSAGE,
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

#[tokio::test]
async fn test_start_grpc_server() {
    let consensus_controller = MockConsensusControllerImpl::new();
    let execution_ctrl = MockExecutionController::new_with_receiver();
    let shared_storage: massa_storage::Storage = massa_storage::Storage::create_root();
    let selector_ctrl = MockSelectorController::new_with_receiver();
    let pool_ctrl = MockPoolController::new_with_receiver();
    let (consensus_event_sender, _consensus_event_receiver) =
        MassaChannel::new("consensus_event".to_string(), Some(1024));

    let consensus_channels = ConsensusChannels {
        execution_controller: execution_ctrl.0.clone(),
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
        bind: SocketAddr::from(([0, 0, 0, 0], 8888)),
        accept_compressed: None,
        send_compressed: None,
        max_decoding_message_size: 4194304,
        max_encoding_message_size: 4194304,
        max_gas_per_block: u32::MAX as u64,
        concurrency_limit_per_connection: 15,
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
        max_operations_per_message: MAX_OPERATIONS_PER_MESSAGE,
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

    let service = MassaPublicGrpc {
        consensus_controller: Box::new(consensus_controller),
        consensus_channels,
        execution_controller: execution_ctrl.0.clone(),
        execution_channels: ExecutionChannels {
            slot_execution_output_sender,
        },
        pool_channels: PoolChannels {
            endorsement_sender,
            operation_sender,
            selector: selector_ctrl.0.clone(),
            execution_controller: execution_ctrl.0.clone(),
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
    };

    let stop_handle = service.serve(&grpc_config).await.unwrap();
    // std::thread::sleep(Duration::from_millis(100));

    // start grpc client and connect to the server
    let channel = tonic::transport::Channel::from_static("grpc://localhost:8888")
        .connect()
        .await
        .unwrap();

    let mut res = PublicServiceClient::new(channel);

    let s = res.get_status(GetStatusRequest {}).await.unwrap();
    dbg!(s);
    stop_handle.stop();
    std::thread::sleep(Duration::from_millis(1000));
}

#[tokio::test]
async fn get_status() {
    let mut public_server = grpc_public_service();

    let mut exec_ctrl = MockExecutionCtrl::new();
    exec_ctrl
        .expect_query_state()
        .returning(|_| massa_execution_exports::ExecutionQueryResponse {
            responses: vec![],
            candidate_cursor: massa_models::slot::Slot::new(0, 2),
            final_cursor: Slot::new(0, 0),
            final_state_fingerprint: massa_hash::Hash::compute_from(&Vec::new()),
        });

    public_server.execution_controller = Box::new(exec_ctrl);

    let config = public_server.grpc_config.clone();
    let stop_handle = public_server.serve(&config).await.unwrap();
    // start grpc client and connect to the server
    let mut public_client = PublicServiceClient::connect("grpc://localhost:8888")
        .await
        .unwrap();
    let response = public_client.get_status(GetStatusRequest {}).await.unwrap();
    let result = response.into_inner();
    assert_eq!(result.status.unwrap().version, *VERSION.to_string());

    stop_handle.stop();
    std::thread::sleep(Duration::from_millis(1000));
}

#[tokio::test]
async fn get_transactions_throughput() {
    let mut public_server = grpc_public_service();

    let mut exec_ctrl = MockExecutionCtrl::new();
    exec_ctrl.expect_get_stats().returning(|| ExecutionStats {
        time_window_start: MassaTime::now().unwrap(),
        time_window_end: MassaTime::now().unwrap(),
        final_block_count: 0,
        final_executed_operations_count: 0,
        active_cursor: Slot::new(0, 0),
        final_cursor: Slot::new(0, 0),
    });

    public_server.execution_controller = Box::new(exec_ctrl);

    let config = public_server.grpc_config.clone();
    let stop_handle = public_server.serve(&config).await.unwrap();
    // start grpc client and connect to the server
    let mut public_client = PublicServiceClient::connect("grpc://localhost:8888")
        .await
        .unwrap();
    let response = public_client
        .get_transactions_throughput(GetTransactionsThroughputRequest {})
        .await
        .unwrap()
        .into_inner();

    assert_eq!(response.throughput, 0);
    stop_handle.stop();
    std::thread::sleep(Duration::from_millis(1000));
}

#[tokio::test]
async fn get_operations() {
    let mut public_server = grpc_public_service();
    let config = public_server.grpc_config.clone();

    // create an operation and store it in the storage
    let op = create_operation_with_expire_period(&KeyPair::generate(0).unwrap(), 0);
    let op_id = op.id.clone();
    public_server.storage.store_operations(vec![op]);

    std::thread::sleep(Duration::from_millis(1000));

    // start the server
    let stop_handle = public_server.serve(&config).await.unwrap();

    // start grpc client and connect to the server
    let mut public_client = PublicServiceClient::connect("grpc://localhost:8888")
        .await
        .unwrap();

    let response = public_client
        .get_operations(GetOperationsRequest {
            operation_ids: vec![op_id.to_string()],
        })
        .await
        .unwrap()
        .into_inner();

    let op_type = response
        .wrapped_operations
        .get(0)
        .unwrap()
        .clone()
        .operation
        .unwrap()
        .clone()
        .content
        .unwrap()
        .op
        .unwrap();

    match op_type.r#type.unwrap() {
        massa_proto_rs::massa::model::v1::operation_type::Type::Transaction(_) => (),
        _ => panic!("wrong operation type"),
    }
    stop_handle.stop();
    std::thread::sleep(Duration::from_millis(1000));
}

#[tokio::test]
async fn get_blocks() {
    let mut public_server = grpc_public_service();
    let mut consensus_ctrl = MockConsensusControllerImpl::new();
    consensus_ctrl
        .expect_get_block_statuses()
        .returning(|_| vec![BlockGraphStatus::Final]);

    public_server.consensus_controller = Box::new(consensus_ctrl);

    let config = public_server.grpc_config.clone();

    let block = create_block(&KeyPair::generate(0).unwrap());
    public_server.storage.store_block(block.clone());

    // start the server
    let stop_handle = public_server.serve(&config).await.unwrap();

    // start grpc client and connect to the server
    let mut public_client = PublicServiceClient::connect(GRPC_SERVER_URL).await.unwrap();

    let result = public_client
        .get_blocks(GetBlocksRequest {
            block_ids: vec![block.id.to_string()],
        })
        .await
        .unwrap()
        .into_inner();

    let s = result.wrapped_blocks.get(0).unwrap().clone().status;

    assert_eq!(s, BlockStatus::Final as i32);
    stop_handle.stop();
    std::thread::sleep(Duration::from_millis(1000));
}

#[tokio::test]
async fn get_stakers() {
    let mut public_server = grpc_public_service();

    let mut exec_ctrl = MockExecutionCtrl::new();
    exec_ctrl.expect_get_cycle_active_rolls().returning(|_| {
        let mut map = std::collections::BTreeMap::new();
        map.insert(
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
            5 as u64,
        );
        map.insert(
            Address::from_str("AU12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G").unwrap(),
            10 as u64,
        );
        map.insert(
            Address::from_str("AU12cMW9zRKFDS43Z2W88VCmdQFxmHjAo54XvuVV34UzJeXRLXW9M").unwrap(),
            20 as u64,
        );
        map.insert(
            Address::from_public_key(&KeyPair::generate(0).unwrap().get_public_key()),
            30 as u64,
        );

        map
    });

    public_server.execution_controller = Box::new(exec_ctrl);
    let config = public_server.grpc_config.clone();

    // start the server
    let stop_handle = public_server.serve(&config).await.unwrap();
    // start grpc client and connect to the server
    let mut public_client = PublicServiceClient::connect(GRPC_SERVER_URL).await.unwrap();

    let result = public_client
        .get_stakers(massa_proto_rs::massa::api::v1::GetStakersRequest {
            filters: vec![massa_proto_rs::massa::api::v1::StakersFilter {
                filter: Some(massa_proto_rs::massa::api::v1::stakers_filter::Filter::MinRolls(20)),
            }],
        })
        .await
        .unwrap()
        .into_inner();

    result.stakers.iter().for_each(|s| {
        assert!(s.rolls >= 20);
    });

    let result = public_client
        .get_stakers(massa_proto_rs::massa::api::v1::GetStakersRequest {
            filters: vec![massa_proto_rs::massa::api::v1::StakersFilter {
                filter: Some(massa_proto_rs::massa::api::v1::stakers_filter::Filter::MaxRolls(20)),
            }],
        })
        .await
        .unwrap()
        .into_inner();

    result.stakers.iter().for_each(|s| {
        assert!(s.rolls <= 20);
    });

    assert_eq!(result.stakers.len(), 3);

    let result = public_client
        .get_stakers(massa_proto_rs::massa::api::v1::GetStakersRequest {
            filters: vec![massa_proto_rs::massa::api::v1::StakersFilter {
                filter: Some(massa_proto_rs::massa::api::v1::stakers_filter::Filter::Limit(2)),
            }],
        })
        .await
        .unwrap()
        .into_inner();

    assert_eq!(result.stakers.len(), 2);
    stop_handle.stop();
}

#[tokio::test]
async fn get_datastore_entries() {
    let mut public_server = grpc_public_service();

    let mut exec_ctrl = MockExecutionCtrl::new();
    exec_ctrl
        .expect_get_final_and_active_data_entry()
        .returning(|_| vec![(Some("toto".as_bytes().to_vec()), None)]);

    public_server.execution_controller = Box::new(exec_ctrl);
    let config = public_server.grpc_config.clone();

    // start the server
    let stop_handle = public_server.serve(&config).await.unwrap();
    // start grpc client and connect to the server
    let mut public_client = PublicServiceClient::connect(GRPC_SERVER_URL).await.unwrap();

    let result = public_client
        .get_datastore_entries(massa_proto_rs::massa::api::v1::GetDatastoreEntriesRequest {
            filters: vec![massa_proto_rs::massa::api::v1::GetDatastoreEntryFilter {
                filter: Some(Filter::AddressKey(
                    massa_proto_rs::massa::model::v1::AddressKeyEntry {
                        address: "AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x"
                            .to_string(),
                        key: vec![],
                    },
                )),
            }],
        })
        .await
        .unwrap()
        .into_inner();

    let data = result.datastore_entries.get(0).unwrap();
    // TODO candidate value should be an option in the api
    assert!(data.candidate_value.is_empty());
    assert_eq!(data.final_value, "toto".as_bytes());

    stop_handle.stop();
}

#[tokio::test]
async fn execute_read_only_call() {
    let mut public_server = grpc_public_service();
    let config = public_server.grpc_config.clone();

    let mut exec_ctrl = MockExecutionCtrl::new();
    exec_ctrl
        .expect_execute_readonly_request()
        .returning(|_req| {
            Ok(massa_execution_exports::ReadOnlyExecutionOutput {
                out: massa_execution_exports::ExecutionOutput {
                    slot: Slot {
                        period: 1,
                        thread: 5,
                    },
                    block_info: None,
                    state_changes: massa_final_state::StateChanges::default(),
                    events: EventStore::default(),
                },
                gas_cost: 100,
                call_result: "toto".as_bytes().to_vec(),
            })
        });

    public_server.execution_controller = Box::new(exec_ctrl);

    let stop_handle = public_server.serve(&config).await.unwrap();
    // start grpc client and connect to the server
    let mut public_client = PublicServiceClient::connect(GRPC_SERVER_URL).await.unwrap();

    let mut param = ReadOnlyExecutionCall {
        max_gas: u64::MAX,
        call_stack: vec![],
        caller_address: None,
        is_final: false,
        target: Some(Target::FunctionCall(FunctionCall {
            target_address: "toto".to_string(),
            target_function: "function".to_string(),
            parameter: vec![],
        })),
    };

    let call = public_client
        .execute_read_only_call(ExecuteReadOnlyCallRequest {
            call: Some(param.clone()),
        })
        .await;

    assert!(call.is_err());

    param.target = Some(Target::FunctionCall(FunctionCall {
        target_address: "AS12cx6BJHSrBPPSE86E6LYgYS44dvXoHW77cdPbTT8H41wm6xGN5".to_string(),
        target_function: "function".to_string(),
        parameter: vec![],
    }));

    let call = public_client
        .execute_read_only_call(ExecuteReadOnlyCallRequest { call: Some(param) })
        .await
        .unwrap()
        .into_inner();

    dbg!(&call);

    assert_eq!(call.clone().output.unwrap().call_result, "toto".as_bytes());
    assert_eq!(call.output.unwrap().used_gas, 100);

    stop_handle.stop();
}
