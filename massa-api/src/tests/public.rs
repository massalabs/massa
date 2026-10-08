//! Copyright (c) 2023 MASSA LABS <info@massa.net>
//!

use std::{
    collections::{BTreeMap, HashMap},
    net::{IpAddr, SocketAddr},
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};

use jsonrpsee::{
    core::{client::ClientT, client::Error},
    http_client::HttpClientBuilder,
    rpc_params,
};
use massa_api_exports::{
    address::{AddressFilter, AddressInfo},
    block::{BlockInfo, BlockSummary},
    datastore::{DatastoreEntryInput, DatastoreEntryOutput},
    endorsement::EndorsementInfo,
    execution::{ExecuteReadOnlyResponse, ReadOnlyBytecodeExecution, ReadOnlyCall},
    operation::{OperationInfo, OperationInput},
    TimeInterval,
};
use massa_consensus_exports::{
    block_graph_export::BlockGraphExport, block_status::ExportCompiledBlock,
    MockConsensusController,
};
use massa_pool_exports::MockPoolController;
use massa_pos_exports::MockSelectorController;

use crate::{tests::mock::start_public_api, MassaRpcServer, RpcServer};
use massa_execution_exports::{
    ExecutionAddressInfo, ExecutionQueryRequest, ExecutionQueryRequestItem, ExecutionQueryResponse,
    ExecutionQueryResponseItem, MockExecutionController, ReadOnlyExecutionOutput,
};
use massa_models::{
    address::Address,
    amount::Amount,
    block::{Block, BlockGraphStatus},
    bytecode::Bytecode,
    clique::Clique,
    endorsement::EndorsementId,
    execution::EventFilter,
    node::NodeId,
    operation::OperationId,
    output_event::SCOutputEvent,
    prehash::{CapacityAllocator, PreHashMap},
    slot::Slot,
    stats::{ConsensusStats, ExecutionStats, NetworkStats},
};
use massa_protocol_exports::{
    test_exports::tools::{
        create_block, create_call_sc_op_with_too_much_gas, create_endorsement,
        create_execute_sc_op_with_too_much_gas, create_operation_with_expire_period,
    },
    MockProtocolController,
};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use serde_json::Value;
use tempfile::NamedTempFile;

#[tokio::test]
async fn get_status() {
    let addr: SocketAddr = "[::]:5001".parse().unwrap();
    let (mut api_public, config) = start_public_api(addr);

    let mut exec_ctrl = MockExecutionController::new();

    exec_ctrl.expect_get_stats().returning(|| ExecutionStats {
        time_window_start: MassaTime::now(),
        time_window_end: MassaTime::now(),
        final_block_count: 0,
        final_executed_operations_count: 0,
        active_cursor: Slot::new(0, 0),
        final_cursor: Slot::new(0, 0),
    });

    let mut consensus_ctrl = MockConsensusController::new();
    consensus_ctrl.expect_get_stats().returning(|| {
        Ok(ConsensusStats {
            start_timespan: MassaTime::now(),
            end_timespan: MassaTime::now(),
            final_block_count: 50,
            stale_block_count: 40,
            clique_count: 30,
        })
    });

    let mut protocol_ctrl = MockProtocolController::new();
    protocol_ctrl.expect_get_stats().returning(|| {
        Ok((
            NetworkStats {
                in_connection_count: 10,
                out_connection_count: 5,
                known_peer_count: 6,
                banned_peer_count: 0,
                active_node_count: 15,
            },
            HashMap::new(),
        ))
    });

    let mut pool_ctrl = MockPoolController::new();
    pool_ctrl
        .expect_get_operation_count()
        .returning(|_| Ok(1024));
    pool_ctrl
        .expect_get_endorsement_count()
        .returning(|_| Ok(2048));

    api_public.0.pool_command_sender = Box::new(pool_ctrl);
    api_public.0.protocol_controller = Box::new(protocol_ctrl);
    api_public.0.execution_controller = Box::new(exec_ctrl);
    api_public.0.consensus_controller = Box::new(consensus_ctrl);

    let api_public_handle = api_public
        .serve(&addr, &config)
        .await
        .expect("failed to start PUBLIC API");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').next_back().unwrap()
        ))
        .unwrap();
    let params = rpc_params![];
    let response: massa_api_exports::node::NodeStatus =
        client.request("get_status", params).await.unwrap();

    assert_eq!(response.network_stats.in_connection_count, 10);
    assert_eq!(response.network_stats.out_connection_count, 5);
    assert_eq!(response.config.thread_count, 32);
    // Chain id == 77 for Node in sandbox mode otherwise it is always greater
    assert!(response.chain_id >= 77);
    assert_eq!(
        response.max_datastore_keys_query,
        config.max_datastore_keys_queries
    );

    api_public_handle.stop().await;
}

#[tokio::test]
async fn get_cliques() {
    let addr: SocketAddr = "[::]:5002".parse().unwrap();
    let (mut api_public, config) = start_public_api(addr);

    let mut consensus_ctrl = MockConsensusController::new();
    consensus_ctrl
        .expect_get_cliques()
        .returning(|| vec![Clique::default()]);

    api_public.0.consensus_controller = Box::new(consensus_ctrl);

    let api_public_handle = api_public
        .serve(&addr, &config)
        .await
        .expect("failed to start PUBLIC API");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').next_back().unwrap()
        ))
        .unwrap();
    let params = rpc_params![];
    let response: Vec<massa_models::clique::Clique> =
        client.request("get_cliques", params).await.unwrap();

    assert_eq!(response.len(), 1);

    api_public_handle.stop().await;
}

#[tokio::test]
async fn get_operations() {
    let addr: SocketAddr = "[::]:5003".parse().unwrap();
    let (mut api_public, config) = start_public_api(addr);
    let keypair = KeyPair::generate(0).unwrap();
    let op = create_operation_with_expire_period(&keypair, 500000);

    api_public.0.storage.store_operations(vec![op.clone()]);

    let mut pool_ctrl = MockPoolController::new();
    pool_ctrl
        .expect_contains_operations()
        .returning(|ids, _| Ok(ids.iter().map(|_id| true).collect()));

    let mut exec_ctrl = MockExecutionController::new();
    exec_ctrl
        .expect_get_ops_exec_status()
        .returning(|op| op.iter().map(|_op| (Some(true), Some(true))).collect());

    api_public.0.execution_controller = Box::new(exec_ctrl);
    api_public.0.pool_command_sender = Box::new(pool_ctrl);

    let api_public_handle = api_public
        .serve(&addr, &config)
        .await
        .expect("failed to start PUBLIC API");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').next_back().unwrap()
        ))
        .unwrap();
    let params = rpc_params![vec![
        OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap(),
        op.id
    ]];
    let response: Vec<OperationInfo> = client.request("get_operations", params).await.unwrap();

    assert_eq!(response.len(), 1);

    api_public_handle.stop().await;
}

#[tokio::test]
async fn get_endorsements() {
    let addr: SocketAddr = "[::]:5005".parse().unwrap();
    let (mut api_public, config) = start_public_api(addr);

    let end = create_endorsement();
    api_public.0.storage.store_endorsements(vec![end.clone()]);

    let mut pool_ctrl = MockPoolController::new();
    pool_ctrl
        .expect_contains_endorsements()
        .returning(|ids, _| Ok(ids.iter().map(|_| true).collect::<Vec<bool>>()));

    let mut consensus_ctrl = MockConsensusController::new();
    consensus_ctrl
        .expect_get_block_statuses()
        .returning(|param| param.iter().map(|_| BlockGraphStatus::Final).collect());

    api_public.0.consensus_controller = Box::new(consensus_ctrl);
    api_public.0.pool_command_sender = Box::new(pool_ctrl);

    let api_public_handle = api_public
        .serve(&addr, &config)
        .await
        .expect("failed to start PUBLIC API");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').next_back().unwrap()
        ))
        .unwrap();

    let params = rpc_params![];
    let response: Result<Vec<EndorsementInfo>, Error> =
        client.request("get_endorsements", params.clone()).await;
    assert!(response.unwrap_err().to_string().contains("Invalid params"));

    let response: Vec<EndorsementInfo> = client
        .request(
            "get_endorsements",
            rpc_params![vec![EndorsementId::from_str(
                "E19dHCWcodoSppzEZbGccshMhNSxYDTFGthqo5LRa4QyaQbL8cw"
            )
            .unwrap()]],
        )
        .await
        .unwrap();
    assert!(response.is_empty());

    let response: Vec<EndorsementInfo> = client
        .request("get_endorsements", rpc_params![vec![end.id]])
        .await
        .unwrap();
    assert!(response.len() == 1);

    api_public_handle.stop().await;
}

#[tokio::test]
async fn get_blocks() {
    let addr: SocketAddr = "[::]:5006".parse().unwrap();
    let (mut api_public, config) = start_public_api(addr);
    let keypair = KeyPair::generate(0).unwrap();
    let block = create_block(&keypair);

    api_public.0.storage.store_block(block.clone());

    let mut consensus_ctrl = MockConsensusController::new();
    consensus_ctrl
        .expect_get_block_statuses()
        .returning(|param| param.iter().map(|_| BlockGraphStatus::Final).collect());

    api_public.0.consensus_controller = Box::new(consensus_ctrl);

    let api_public_handle = api_public
        .serve(&addr, &config)
        .await
        .expect("failed to start PUBLIC API");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').next_back().unwrap()
        ))
        .unwrap();

    let params = rpc_params![];
    let response: Result<Vec<BlockInfo>, Error> =
        client.request("get_blocks", params.clone()).await;
    assert!(response.unwrap_err().to_string().contains("Invalid params"));

    let response: Vec<BlockInfo> = client
        .request("get_blocks", rpc_params![vec![block.id]])
        .await
        .unwrap();

    assert_eq!(response[0].id, block.id);

    api_public_handle.stop().await;
}

#[tokio::test]
async fn get_blockclique_block_by_slot() {
    let addr: SocketAddr = "[::]:5007".parse().unwrap();
    let (mut api_public, config) = start_public_api(addr);

    let block = create_block(&KeyPair::generate(0).unwrap());
    let id = block.id;

    api_public.0.storage.store_block(block.clone());

    let mut consensus_ctrl = MockConsensusController::new();
    consensus_ctrl
        .expect_get_blockclique_block_at_slot()
        .returning(move |_s| Some(id));

    api_public.0.consensus_controller = Box::new(consensus_ctrl);

    let api_public_handle = api_public
        .serve(&addr, &config)
        .await
        .expect("failed to start PUBLIC API");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').next_back().unwrap()
        ))
        .unwrap();

    let params = rpc_params![];
    let response: Result<Option<Block>, Error> = client
        .request("get_blockclique_block_by_slot", params.clone())
        .await;

    assert!(response.unwrap_err().to_string().contains("Invalid params"));
    let response: Option<Block> = client
        .request(
            "get_blockclique_block_by_slot",
            rpc_params![Slot {
                period: 1,
                thread: 0
            }],
        )
        .await
        .unwrap();

    assert!(response.is_some());
    api_public_handle.stop().await;
}

#[tokio::test]
async fn get_graph_interval() {
    let addr: SocketAddr = "[::]:5008".parse().unwrap();
    let (mut api_public, config) = start_public_api(addr);

    let mut consensus_ctrl = MockConsensusController::new();
    consensus_ctrl
        .expect_get_block_graph_status()
        .returning(|_start, _end| {
            let block = create_block(&KeyPair::generate(0).unwrap());
            let id = block.id;

            let mut active = PreHashMap::with_capacity(1);
            active.insert(
                id,
                ExportCompiledBlock {
                    header: block.content.header.clone(),
                    children: vec![],
                    is_final: false,
                },
            );

            let mut discarded = PreHashMap::with_capacity(1);
            discarded.insert(
                id,
                (
                    massa_consensus_exports::block_status::DiscardReason::Invalid(
                        "invalid".to_string(),
                    ),
                    (
                        Slot::new(0, 0),
                        Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x")
                            .unwrap(),
                        vec![],
                    ),
                ),
            );

            discarded.insert(
                id,
                (
                    massa_consensus_exports::block_status::DiscardReason::Stale,
                    (
                        Slot::new(0, 0),
                        Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x")
                            .unwrap(),
                        vec![],
                    ),
                ),
            );
            Ok(BlockGraphExport {
                genesis_blocks: vec![],
                active_blocks: active,
                discarded_blocks: discarded,
                best_parents: vec![],
                latest_final_blocks_periods: vec![],
                gi_head: PreHashMap::with_capacity(1),
                max_cliques: vec![Clique::default()],
            })
        });

    api_public.0.consensus_controller = Box::new(consensus_ctrl);

    let api_public_handle = api_public
        .serve(&addr, &config)
        .await
        .expect("failed to start PUBLIC API");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').next_back().unwrap()
        ))
        .unwrap();

    let params = rpc_params![TimeInterval {
        start: Some(MassaTime::now()),
        end: Some(MassaTime::now())
    }];
    let response: Vec<BlockSummary> = client
        .request("get_graph_interval", params.clone())
        .await
        .unwrap();
    assert!(response.len() == 2);
    api_public_handle.stop().await;
}

#[tokio::test]
async fn send_operations_low_fee() {
    let addr: SocketAddr = "[::]:5049".parse().unwrap();
    let (mut api_public, mut config) = start_public_api(addr);

    config.minimal_fees = Amount::from_str("0.01").unwrap();
    api_public.0.api_settings.minimal_fees = Amount::from_str("0.01").unwrap();

    let mut pool_ctrl = MockPoolController::new();
    pool_ctrl.expect_clone_box().returning(|| {
        let mut pool_ctrl = MockPoolController::new();
        pool_ctrl.expect_add_operations().returning(|_a| Ok(()));
        Box::new(pool_ctrl)
    });

    let mut protocol_ctrl = MockProtocolController::new();
    protocol_ctrl.expect_clone_box().returning(|| {
        let mut protocol_ctrl = MockProtocolController::new();
        protocol_ctrl
            .expect_propagate_operations()
            .returning(|_a| Ok(()));
        Box::new(protocol_ctrl)
    });

    api_public.0.protocol_controller = Box::new(protocol_ctrl);
    api_public.0.pool_command_sender = Box::new(pool_ctrl);

    let api_public_handle = api_public
        .serve(&addr, &config)
        .await
        .expect("failed to start PUBLIC API");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').next_back().unwrap()
        ))
        .unwrap();
    let keypair = KeyPair::generate(0).unwrap();

    // send transaction
    let operation = create_operation_with_expire_period(&keypair, u64::MAX);

    let input: OperationInput = OperationInput {
        creator_public_key: keypair.get_public_key(),
        signature: operation.signature,
        serialized_content: operation.serialized_data,
    };

    let response: Result<Vec<OperationId>, Error> = client
        .request("send_operations", rpc_params![vec![input]])
        .await;

    let err = response.unwrap_err();

    // op has low fee and should not be executed
    assert!(err
        .to_string()
        .contains("Bad request: fee is too low provided: 0"));

    api_public_handle.stop().await;
}

#[tokio::test]
async fn send_operations() {
    let addr: SocketAddr = "[::]:5014".parse().unwrap();
    let (mut api_public, config) = start_public_api(addr);

    let mut pool_ctrl = MockPoolController::new();
    pool_ctrl.expect_clone_box().returning(|| {
        let mut pool_ctrl = MockPoolController::new();
        pool_ctrl.expect_add_operations().returning(|_a| Ok(()));
        Box::new(pool_ctrl)
    });

    let mut protocol_ctrl = MockProtocolController::new();
    protocol_ctrl.expect_clone_box().returning(|| {
        let mut protocol_ctrl = MockProtocolController::new();
        protocol_ctrl
            .expect_propagate_operations()
            .returning(|_a| Ok(()));
        Box::new(protocol_ctrl)
    });

    api_public.0.protocol_controller = Box::new(protocol_ctrl);
    api_public.0.pool_command_sender = Box::new(pool_ctrl);

    let api_public_handle = api_public
        .serve(&addr, &config)
        .await
        .expect("failed to start PUBLIC API");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').next_back().unwrap()
        ))
        .unwrap();
    let keypair = KeyPair::generate(0).unwrap();

    ////
    // send transaction
    let operation = create_operation_with_expire_period(&keypair, u64::MAX);

    let input: OperationInput = OperationInput {
        creator_public_key: keypair.get_public_key(),
        signature: operation.signature,
        serialized_content: operation.serialized_data,
    };

    let response: Vec<OperationId> = client
        .request("send_operations", rpc_params![vec![input]])
        .await
        .unwrap();

    assert_eq!(response.len(), 1);

    ////
    // send ExecuteSC with too much gas and check error message

    let operation = create_execute_sc_op_with_too_much_gas(&keypair, 10);
    let input: OperationInput = OperationInput {
        creator_public_key: keypair.get_public_key(),
        signature: operation.signature,
        serialized_content: operation.serialized_data,
    };

    let response: Result<Vec<OperationId>, _> = client
        .request("send_operations", rpc_params![vec![input]])
        .await;
    let err = response.unwrap_err();
    assert!(err
        .to_string()
        .contains("Upper gas limit for ExecuteSC operation is"));
    println!("{}", err);

    ////
    // send CallSC with too much gas and check error message

    let operation = create_call_sc_op_with_too_much_gas(&keypair, 10);
    let input: OperationInput = OperationInput {
        creator_public_key: keypair.get_public_key(),
        signature: operation.signature,
        serialized_content: operation.serialized_data,
    };

    let response: Result<Vec<OperationId>, _> = client
        .request("send_operations", rpc_params![vec![input]])
        .await;
    let err = response.unwrap_err();
    assert!(err
        .to_string()
        .contains("Upper gas limit for CallSC operation is"));
    println!("{}", err);

    api_public_handle.stop().await;
}

#[tokio::test]
async fn get_filtered_sc_output_event() {
    let addr: SocketAddr = "[::]:5013".parse().unwrap();
    let (mut api_public, config) = start_public_api(addr);

    let mut exec_ctrl = MockExecutionController::new();
    exec_ctrl
        .expect_get_filtered_sc_output_event()
        .returning(|_a| {
            vec![SCOutputEvent {
                context: massa_models::output_event::EventExecutionContext {
                    slot: Slot {
                        period: 1,
                        thread: 10,
                    },
                    block: None,
                    read_only: false,
                    index_in_slot: 1,
                    call_stack: std::collections::VecDeque::new(),
                    origin_operation_id: Some(
                        massa_models::operation::OperationId::from_str(
                            "O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC",
                        )
                        .unwrap(),
                    ),
                    is_final: false,
                    is_error: false,
                    deferred_call_id: None,
                    async_msg_id: None,
                },
                data: "massa".to_string(),
            }]
        });

    api_public.0.execution_controller = Box::new(exec_ctrl);
    let api_public_handle = api_public
        .serve(&addr, &config)
        .await
        .expect("failed to start PUBLIC API");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').next_back().unwrap()
        ))
        .unwrap();

    let response: Result<Vec<SCOutputEvent>, Error> = client
        .request("get_filtered_sc_output_event", rpc_params![])
        .await;

    // assert invalid params
    assert!(response.unwrap_err().to_string().contains("Invalid params"));

    let response: Result<Vec<SCOutputEvent>, Error> = client
        .request(
            "get_filtered_sc_output_event",
            rpc_params![EventFilter {
                start: Some(Slot {
                    period: 1,
                    thread: 1
                }),
                ..Default::default()
            }],
        )
        .await;

    assert_eq!(response.unwrap().len(), 1);
    api_public_handle.stop().await;
}

#[tokio::test]
async fn execute_read_only_bytecode() {
    let addr: SocketAddr = "[::]:5012".parse().unwrap();
    let (mut api_public, config) = start_public_api(addr);

    let mut exec_ctrl = MockExecutionController::new();
    exec_ctrl
        .expect_execute_readonly_request()
        .returning(|_req| {
            Ok(ReadOnlyExecutionOutput {
                out: massa_execution_exports::ExecutionOutput {
                    slot: Slot {
                        period: 1,
                        thread: 5,
                    },
                    block_info: None,
                    state_changes: massa_final_state::StateChanges::default(),
                    events: massa_execution_exports::EventStore::default(),
                    #[cfg(feature = "execution-trace")]
                    slot_trace: None,
                    #[cfg(feature = "dump-block")]
                    storage: None,
                    deferred_credits_execution: vec![],
                    cancel_async_message_execution: vec![],
                    auto_sell_execution: vec![],
                    transfers_history: Default::default(),
                    execution_info: None,
                },
                gas_cost: 100,
                call_result: "toto".as_bytes().to_vec(),
            })
        });

    api_public.0.execution_controller = Box::new(exec_ctrl);
    let api_public_handle = api_public
        .serve(&addr, &config)
        .await
        .expect("failed to start PUBLIC API");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').next_back().unwrap()
        ))
        .unwrap();

    let params = rpc_params![vec![ReadOnlyBytecodeExecution {
        max_gas: 100000,
        bytecode: "hi".as_bytes().to_vec(),
        address: Some(
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap()
        ),
        operation_datastore: None,
        fee: None
    }]];
    let response: Result<Vec<ExecuteReadOnlyResponse>, Error> = client
        .request("execute_read_only_bytecode", params.clone())
        .await;

    assert!(response.unwrap().len() == 1);

    let params = rpc_params![vec![ReadOnlyBytecodeExecution {
        max_gas: 100000,
        bytecode: "hi".as_bytes().to_vec(),
        address: None,
        operation_datastore: None,
        fee: None,
    }]];
    let response: Result<Vec<ExecuteReadOnlyResponse>, Error> = client
        .request("execute_read_only_bytecode", params.clone())
        .await;

    assert!(response.unwrap().len() == 1);

    let params = rpc_params![vec![ReadOnlyBytecodeExecution {
        max_gas: 100000,
        bytecode: "hi".as_bytes().to_vec(),
        address: None,
        operation_datastore: Some("hi".as_bytes().to_vec()),
        fee: None
    }]];
    let response: Result<Vec<ExecuteReadOnlyResponse>, Error> = client
        .request("execute_read_only_bytecode", params.clone())
        .await;

    assert!(response.is_err());
    api_public_handle.stop().await;
}

#[tokio::test]
async fn execute_read_only_call() {
    let addr: SocketAddr = "[::]:5011".parse().unwrap();
    let (mut api_public, config) = start_public_api(addr);

    let mut exec_ctrl = MockExecutionController::new();
    exec_ctrl
        .expect_execute_readonly_request()
        .returning(|_req| {
            Ok(ReadOnlyExecutionOutput {
                out: massa_execution_exports::ExecutionOutput {
                    slot: Slot {
                        period: 1,
                        thread: 5,
                    },
                    block_info: None,
                    state_changes: massa_final_state::StateChanges::default(),
                    events: massa_execution_exports::EventStore::default(),
                    #[cfg(feature = "execution-trace")]
                    slot_trace: None,
                    #[cfg(feature = "dump-block")]
                    storage: None,
                    deferred_credits_execution: vec![],
                    cancel_async_message_execution: vec![],
                    auto_sell_execution: vec![],
                    transfers_history: Default::default(),
                    execution_info: None,
                },
                gas_cost: 100,
                call_result: "toto".as_bytes().to_vec(),
            })
        });

    api_public.0.execution_controller = Box::new(exec_ctrl);

    let api_public_handle = api_public
        .serve(&addr, &config)
        .await
        .expect("failed to start PUBLIC API");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').next_back().unwrap()
        ))
        .unwrap();

    let params = rpc_params![];
    let response: Result<Vec<ExecuteReadOnlyResponse>, Error> = client
        .request("execute_read_only_call", params.clone())
        .await;
    assert!(response.unwrap_err().to_string().contains("Invalid params"));

    let params = rpc_params![vec![ReadOnlyCall {
        max_gas: 1000000,
        target_address: Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x")
            .unwrap(),
        target_function: "hello".to_string(),
        parameter: vec![],
        caller_address: None,
        fee: None,
        coins: None,
    }]];
    let response: Vec<ExecuteReadOnlyResponse> = client
        .request("execute_read_only_call", params.clone())
        .await
        .unwrap();

    assert_eq!(response.len(), 1);
    api_public_handle.stop().await;
}

#[tokio::test]
async fn get_addresses() {
    let addr: SocketAddr = "[::]:5010".parse().unwrap();
    let (mut api_public, config) = start_public_api(addr);

    let mut exec_ctrl = MockExecutionController::new();
    exec_ctrl
        .expect_get_addresses_infos()
        .returning(|a, _s, _m| {
            a.iter()
                .map(|_addr| ExecutionAddressInfo {
                    candidate_balance: Amount::from_str("100000").unwrap(),
                    final_balance: Amount::from_str("80000").unwrap(),
                    final_roll_count: 55,
                    final_datastore_keys: std::collections::BTreeSet::new(),
                    candidate_roll_count: 12,
                    candidate_datastore_keys: std::collections::BTreeSet::new(),
                    future_deferred_credits: BTreeMap::new(),
                    cycle_infos: vec![],
                })
                .collect()
        });

    let mut selector_ctrl = MockSelectorController::new();
    selector_ctrl
        .expect_get_available_selections_in_range()
        .returning(|_range, _addrs| Ok(BTreeMap::new()));

    api_public.0.execution_controller = Box::new(exec_ctrl);
    api_public.0.selector_controller = Box::new(selector_ctrl);

    let api_public_handle = api_public
        .serve(&addr, &config)
        .await
        .expect("failed to start PUBLIC API");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').next_back().unwrap()
        ))
        .unwrap();

    let params = rpc_params![];
    let response: Result<Vec<AddressInfo>, Error> =
        client.request("get_addresses", params.clone()).await;
    assert!(response.unwrap_err().to_string().contains("Invalid params"));

    let params = rpc_params![vec![Address::from_str(
        "AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x"
    )
    .unwrap()]];
    let response: Vec<AddressInfo> = client
        .request("get_addresses", params.clone())
        .await
        .unwrap();

    assert!(response.len() == 1);

    api_public_handle.stop().await;
}

#[tokio::test]
async fn get_addresses_bytecode() {
    let addr: SocketAddr = "[::]:5019".parse().unwrap();
    let (mut api_public, config) = start_public_api(addr);

    let mut exec_ctrl: MockExecutionController = MockExecutionController::new();
    exec_ctrl
        .expect_query_state()
        .returning(|_| ExecutionQueryResponse {
            responses: vec![Ok(ExecutionQueryResponseItem::Bytecode(Bytecode(
                "massa".as_bytes().to_vec(),
            )))],
            candidate_cursor: massa_models::slot::Slot::new(1, 2),
            final_cursor: Slot::new(1, 7),
            final_state_fingerprint: massa_hash::Hash::compute_from(&Vec::new()),
        });

    api_public.0.execution_controller = Box::new(exec_ctrl);

    let api_public_handle = api_public
        .serve(&addr, &config)
        .await
        .expect("failed to start PUBLIC API");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').next_back().unwrap()
        ))
        .unwrap();

    let params = rpc_params![];
    let response: Result<Vec<Vec<u8>>, Error> = client
        .request("get_addresses_bytecode", params.clone())
        .await;
    assert!(response.unwrap_err().to_string().contains("Invalid params"));

    let params = rpc_params![vec![AddressFilter {
        address: Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x")
            .unwrap(),
        is_final: true
    }]];
    let response: Vec<Vec<u8>> = client
        .request("get_addresses_bytecode", params.clone())
        .await
        .unwrap();

    assert!(response.len() == 1);

    api_public_handle.stop().await;
}

#[tokio::test]
async fn query_state_response_budget_jsonrpc_settings_and_error() {
    let addr: SocketAddr = "[::]:0".parse().unwrap();
    let (mut api_public, mut config) = start_public_api(addr);
    config.max_response_body_size = 10;
    config.max_event_per_query = 1;
    config.query_state_deadline_ms = Some(0);
    config.max_arguments = 2;
    api_public.0.api_settings = config;

    let address =
        Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap();
    let expected_address = address;
    let mut call_count = 0;
    let mut exec_ctrl = MockExecutionController::new();
    exec_ctrl
        .expect_query_state()
        .times(2)
        .returning(move |request: ExecutionQueryRequest| {
            call_count += 1;
            assert_eq!(request.max_response_size, 10);
            assert_eq!(request.max_event_count, Some(1));
            assert_eq!(request.query_state_deadline_ms, Some(0));
            assert!(matches!(
                request.requests.as_slice(),
                [
                    ExecutionQueryRequestItem::AddressBytecodeFinal(final_address),
                    ExecutionQueryRequestItem::AddressBytecodeCandidate(candidate_address)
                ] if final_address == &expected_address && candidate_address == &expected_address
            ));

            let cursor = Slot::new(1, 2);
            let bytecode = Bytecode(b"abcdef".to_vec());
            ExecutionQueryResponse {
                responses: if call_count == 1 {
                    vec![
                        Ok(ExecutionQueryResponseItem::Bytecode(bytecode)),
                        Err(
                            massa_execution_exports::ExecutionQueryError::TooLargeResponse(
                                "fixture budget exceeded".to_string(),
                            ),
                        ),
                    ]
                } else {
                    vec![
                        Ok(ExecutionQueryResponseItem::Bytecode(bytecode)),
                        Ok(ExecutionQueryResponseItem::Bytecode(Bytecode(
                            b"1234".to_vec(),
                        ))),
                    ]
                },
                candidate_cursor: cursor,
                final_cursor: cursor,
                final_state_fingerprint: massa_hash::Hash::compute_from(&Vec::new()),
            }
        });
    api_public.0.execution_controller = Box::new(exec_ctrl);

    let filters = vec![
        AddressFilter {
            address,
            is_final: true,
        },
        AddressFilter {
            address,
            is_final: false,
        },
    ];
    let error = api_public
        .get_addresses_bytecode(filters.clone())
        .await
        .unwrap_err();
    assert_eq!(error.code(), -32001);
    assert_eq!(
        error.message(),
        "Internal server error: Cumulative response size limit exceeded: fixture budget exceeded"
    );

    let success = api_public.get_addresses_bytecode(filters).await.unwrap();
    assert_eq!(success, vec![b"abcdef".to_vec(), b"1234".to_vec()]);
}

#[tokio::test]
async fn query_state_response_budget_jsonrpc_input_limit() {
    let addr: SocketAddr = "[::]:0".parse().unwrap();
    let (mut api_public, mut config) = start_public_api(addr);
    config.max_arguments = 2;
    api_public.0.api_settings = config;

    let mut exec_ctrl = MockExecutionController::new();
    exec_ctrl.expect_query_state().times(0);
    api_public.0.execution_controller = Box::new(exec_ctrl);

    let error = api_public.get_addresses_bytecode(vec![]).await.unwrap_err();
    assert_eq!(error.code(), -32000);
    assert_eq!(error.message(), "Bad request: no arguments specified");

    let address =
        Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap();
    let error = api_public
        .get_addresses_bytecode(vec![
            AddressFilter {
                address,
                is_final: true,
            };
            3
        ])
        .await
        .unwrap_err();
    assert_eq!(error.code(), -32000);
    assert_eq!(
        error.message(),
        "Bad request: too many arguments received. Only a maximum of 2 arguments are accepted per request"
    );
}

#[tokio::test]
async fn get_datastore_entries() {
    let addr: SocketAddr = "[::]:5009".parse().unwrap();
    let (mut api_public, config) = start_public_api(addr);

    let mut exec_ctrl = MockExecutionController::new();
    exec_ctrl
        .expect_get_final_and_active_data_entry()
        .returning(|_a| {
            vec![(
                Some("massa".as_bytes().to_vec()),
                Some("blockchain".as_bytes().to_vec()),
            )]
        });

    api_public.0.execution_controller = Box::new(exec_ctrl);

    let api_public_handle = api_public
        .serve(&addr, &config)
        .await
        .expect("failed to start PUBLIC API");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').next_back().unwrap()
        ))
        .unwrap();

    let params = rpc_params![vec![DatastoreEntryInput {
        address: Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x")
            .unwrap(),
        key: "massa".as_bytes().to_vec()
    }]];
    let response: Vec<DatastoreEntryOutput> = client
        .request("get_datastore_entries", params.clone())
        .await
        .unwrap();

    let entry = response.first().unwrap();

    assert_eq!(
        entry.candidate_value.as_ref().unwrap(),
        &"blockchain".as_bytes().to_vec()
    );
    assert_eq!(
        entry.final_value.as_ref().unwrap(),
        &"massa".as_bytes().to_vec()
    );
    api_public_handle.stop().await;
}

// Utility fixture: observations measure mock output bytes, not production memory use.
fn install_bounded_datastore_oracle(
    api: &mut crate::API<crate::Public>,
    address: Address,
) -> Arc<Mutex<Vec<(usize, usize)>>> {
    let observations = Arc::new(Mutex::new(Vec::new()));
    let recorded = observations.clone();
    let mut exec_ctrl = MockExecutionController::new();
    exec_ctrl
        .expect_get_final_and_active_data_entry()
        .returning(move |entries| {
            assert!(entries.len() <= 64);
            let bytes = entries
                .iter()
                .filter(|(_, key)| key.as_slice() == b"fixture")
                .count()
                * 2
                * 64
                * 1024;
            let mut batches = recorded.lock().unwrap();
            assert!(
                batches.iter().map(|(_, bytes)| bytes).sum::<usize>() + bytes
                    <= 8 * 1024 * 1024
            );
            let output = entries
                .iter()
                .map(|(addr, key)| {
                    assert_eq!(*addr, address);
                    match key.as_slice() {
                        b"fixture" => {
                            (Some(vec![0; 64 * 1024]), Some(vec![0; 64 * 1024]))
                        }
                        b"missing" => (None, None),
                        _ => panic!("unexpected fixture key"),
                    }
                })
                .collect::<Vec<_>>();
            let materialized = output
                .iter()
                .map(|(final_value, candidate_value)| {
                    final_value.as_ref().map_or(0, Vec::len)
                        + candidate_value.as_ref().map_or(0, Vec::len)
                })
                .sum::<usize>();
            assert_eq!(materialized, bytes);
            batches.push((entries.len(), materialized));
            output
        });
    api.0.execution_controller = Box::new(exec_ctrl);
    observations
}

#[tokio::test]
async fn datastore_rpc_duplicate_materialization_is_bounded_and_preserves_missing_keys(
) {
    let (mut api, config) = start_public_api("127.0.0.1:0".parse().unwrap());
    let address = Address::from_str(
        "AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x",
    )
    .unwrap();
    let observed = install_bounded_datastore_oracle(&mut api, address);
    let module = api.into_rpc();
    for count in [1, 8, 32] {
        let entries = vec![
            DatastoreEntryInput {
                address,
                key: b"fixture".to_vec()
            };
            count
        ];
        let request = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "get_datastore_entries", "params": [entries]
        })
        .to_string();
        assert!(request.len() < config.max_request_body_size as usize);
        // raw_json_request dispatches the real registered method, but has no response ceiling.
        let (response, _) = module.raw_json_request(&request, 1).await.unwrap();
        let response: jsonrpsee::types::Response<
            '_,
            Vec<DatastoreEntryOutput>,
        > = serde_json::from_str(&response).unwrap();
        let output = jsonrpsee::types::ResponseSuccess::try_from(response)
            .unwrap()
            .result;
        assert_eq!(output.len(), count);
        for entry in output {
            assert_eq!(entry.final_value.unwrap(), vec![0; 64 * 1024]);
            assert_eq!(entry.candidate_value.unwrap(), vec![0; 64 * 1024]);
        }
        assert_eq!(
            observed.lock().unwrap().last(),
            Some(&(count, count * 2 * 64 * 1024))
        );
    }
    let request = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "get_datastore_entries", "params": [[
            DatastoreEntryInput { address, key: b"fixture".to_vec() },
            DatastoreEntryInput { address, key: b"missing".to_vec() }
        ]]
    })
    .to_string();
    assert!(request.len() < config.max_request_body_size as usize);
    let (response, _) = module.raw_json_request(&request, 1).await.unwrap();
    let response: jsonrpsee::types::Response<'_, Vec<DatastoreEntryOutput>> =
        serde_json::from_str(&response).unwrap();
    let output = jsonrpsee::types::ResponseSuccess::try_from(response)
        .unwrap()
        .result;
    assert_eq!(output.len(), 2);
    assert_eq!(output[0].final_value.as_ref().unwrap().len(), 64 * 1024);
    assert_eq!(output[0].candidate_value.as_ref().unwrap().len(), 64 * 1024);
    assert!(output[1].final_value.is_none());
    assert!(output[1].candidate_value.is_none());
    assert_eq!(
        *observed.lock().unwrap(),
        vec![
            (1, 2 * 64 * 1024),
            (8, 16 * 64 * 1024),
            (32, 64 * 64 * 1024),
            (2, 2 * 64 * 1024)
        ]
    );
}

#[tokio::test]
async fn datastore_rpc_response_ceiling_rejects_after_bounded_backend_materialization(
) {
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let (mut api, mut config) = start_public_api(addr);
    config.max_request_body_size = 4096;
    config.max_response_body_size = 1024;
    api.0.api_settings = config.clone();
    let address = Address::from_str(
        "AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x",
    )
    .unwrap();
    let observed = install_bounded_datastore_oracle(&mut api, address);
    // Use the same jsonrpsee limits as crate::serve, with an ephemeral isolated-loopback port.
    let server = jsonrpsee::server::ServerBuilder::new()
        .max_request_body_size(config.max_request_body_size)
        .max_response_body_size(config.max_response_body_size)
        .http_only()
        .build(addr)
        .await
        .unwrap();
    let client = HttpClientBuilder::default()
        .request_timeout(Duration::from_secs(5))
        .build(format!("http://{}", server.local_addr().unwrap()))
        .unwrap();
    let handle = server.start(api.into_rpc());
    let entries = vec![
        DatastoreEntryInput {
            address,
            key: b"fixture".to_vec()
        };
        32
    ];
    let request = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "get_datastore_entries", "params": [entries.clone()]
    })
    .to_string();
    assert!(request.len() < config.max_request_body_size as usize);
    let response: Result<Vec<DatastoreEntryOutput>, Error> = client
        .request("get_datastore_entries", rpc_params![entries])
        .await;
    handle.stop().unwrap();
    handle.stopped().await;
    match response.unwrap_err() {
        Error::Call(error) => assert_eq!(
            error.code(),
            jsonrpsee::types::error::OVERSIZED_RESPONSE_CODE
        ),
        other => panic!("expected response-ceiling RPC error, got {other}"),
    }
    assert_eq!(*observed.lock().unwrap(), vec![(32, 2 * 32 * 64 * 1024)]);
}

#[tokio::test]
async fn wrong_api() {
    let addr: SocketAddr = "[::]:5004".parse().unwrap();
    let (api_public, config) = start_public_api(addr);

    let api_public_handle = api_public
        .serve(&addr, &config)
        .await
        .expect("failed to start PUBLIC API");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').next_back().unwrap()
        ))
        .unwrap();

    let params = rpc_params![];
    let response: Result<(), Error> = client.request("stop_node", params.clone()).await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    let response: Result<(), Error> = client
        .request("node_sign_message", rpc_params![Vec::<u8>::new()])
        .await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    let response: Result<(), Error> = client
        .request(
            "remove_staking_addresses",
            rpc_params![Vec::<Address>::new()],
        )
        .await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    let response: Result<(), Error> = client
        .request("get_staking_addresses", params.clone())
        .await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    let response: Result<(), Error> = client
        .request("node_ban_by_ip", rpc_params![Vec::<IpAddr>::new()])
        .await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    let response: Result<(), Error> = client
        .request("node_ban_by_id", rpc_params![Vec::<NodeId>::new()])
        .await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    let response: Result<(), Error> = client
        .request("node_unban_by_ip", rpc_params![Vec::<IpAddr>::new()])
        .await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    let response: Result<(), Error> = client
        .request("node_unban_by_id", rpc_params![Vec::<NodeId>::new()])
        .await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    let response: Result<(), Error> = client.request("node_peers_whitelist", params.clone()).await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    let response: Result<(), Error> = client
        .request(
            "node_add_to_peers_whitelist",
            rpc_params![Vec::<IpAddr>::new()],
        )
        .await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    let response: Result<(), Error> = client
        .request(
            "node_remove_from_peers_whitelist",
            rpc_params![Vec::<IpAddr>::new()],
        )
        .await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    let response: Result<(), Error> = client
        .request(
            "node_bootstrap_whitelist",
            rpc_params![Vec::<IpAddr>::new()],
        )
        .await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    let response: Result<(), Error> = client
        .request("node_bootstrap_whitelist_allow_all", rpc_params![])
        .await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    let response: Result<(), Error> = client
        .request(
            "node_add_to_bootstrap_whitelist",
            rpc_params![Vec::<IpAddr>::new()],
        )
        .await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    let response: Result<(), Error> = client
        .request(
            "node_remove_from_peers_whitelist",
            rpc_params![Vec::<IpAddr>::new()],
        )
        .await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    let response: Result<(), Error> = client
        .request("node_bootstrap_whitelist", rpc_params![])
        .await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    let response: Result<(), Error> = client
        .request("node_bootstrap_whitelist_allow_all", rpc_params![])
        .await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    let response: Result<(), Error> = client
        .request(
            "node_add_to_bootstrap_whitelist",
            rpc_params![Vec::<IpAddr>::new()],
        )
        .await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    let response: Result<(), Error> = client
        .request(
            "node_remove_from_bootstrap_whitelist",
            rpc_params![Vec::<IpAddr>::new()],
        )
        .await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    let response: Result<(), Error> = client
        .request("node_bootstrap_blacklist", rpc_params![])
        .await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    let response: Result<(), Error> = client
        .request(
            "node_add_to_bootstrap_blacklist",
            rpc_params![Vec::<IpAddr>::new()],
        )
        .await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    let response: Result<(), Error> = client
        .request(
            "node_remove_from_bootstrap_blacklist",
            rpc_params![Vec::<IpAddr>::new()],
        )
        .await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    let response: Result<(), Error> = client
        .request("add_staking_secret_keys", rpc_params![Vec::<String>::new()])
        .await;
    assert!(response
        .unwrap_err()
        .to_string()
        .contains("The wrong API (either Public or Private) was called"));

    api_public_handle.stop().await;
}

#[tokio::test]
async fn get_openrpc_spec() {
    let addr: SocketAddr = "[::]:5016".parse().unwrap();
    let (mut api_public, mut config) = start_public_api(addr);

    let open_rpc_file = NamedTempFile::new().unwrap();

    let s = "{
        \"title\": \"Sample Pet Store App\",
        \"version\": \"1.0.1\"
      }";

    serde_json::to_writer_pretty(open_rpc_file.as_file(), s).expect("unable to write ledger file");

    config.openrpc_spec_path = open_rpc_file.path().to_path_buf();
    api_public.0.api_settings.openrpc_spec_path = open_rpc_file.path().to_path_buf();

    let api_public_handle = api_public
        .serve(&addr, &config)
        .await
        .expect("failed to start PUBLIC API");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').next_back().unwrap()
        ))
        .unwrap();
    let params = rpc_params![];

    let response: Value = client.request("rpc.discover", params).await.unwrap();
    assert!(response.as_str().unwrap().contains("Sample Pet Store App"));

    api_public_handle.stop().await;

    let addr: SocketAddr = "[::]:5017".parse().unwrap();
    let (mut api_public, mut config) = start_public_api(addr);

    let open_rpc_file = NamedTempFile::new().unwrap();

    config.openrpc_spec_path = open_rpc_file.path().to_path_buf();
    api_public.0.api_settings.openrpc_spec_path = open_rpc_file.path().to_path_buf();

    let api_public_handle = api_public
        .serve(&addr, &config)
        .await
        .expect("failed to start PUBLIC API");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').next_back().unwrap()
        ))
        .unwrap();
    let params = rpc_params![];

    let response: Result<Value, Error> = client.request("rpc.discover", params).await;

    assert!(response
        .unwrap_err()
        .to_string()
        .contains("failed to parse OpenRPC specification"));
    api_public_handle.stop().await;
}

#[cfg(feature = "execution-trace")]
#[tokio::test]
async fn get_slots_transfers_keeps_positional_alignment() {
    use massa_execution_exports::Transfer as ExecTransfer;

    let addr: SocketAddr = "[::]:5051".parse().unwrap();
    let (mut api_public, config) = start_public_api(addr);

    // Query three slots where the middle one has no blockclique block. It must still occupy
    // its position in the (positional) `Vec<Vec<Transfer>>`, otherwise later slots would be
    // misattributed to the wrong slot.
    let present_a = Slot::new(1, 0);
    let missing = Slot::new(2, 0);
    let present_b = Slot::new(3, 0);

    let block_id = create_block(&KeyPair::generate(0).unwrap()).id;

    let mut consensus_ctrl = MockConsensusController::new();
    consensus_ctrl
        .expect_get_blockclique_block_at_slot()
        .returning(move |slot| {
            if slot == missing {
                None
            } else {
                Some(block_id)
            }
        });

    let sample_addr =
        Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap();
    let op_id =
        OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap();

    let mut exec_ctrl = MockExecutionController::new();
    // Encode the slot period into the transfer amount so we can assert res[i] maps to slots[i].
    exec_ctrl
        .expect_get_slot_abi_call_stack_and_transfers()
        .returning(move |slot| {
            (
                None,
                Some(vec![ExecTransfer {
                    from: sample_addr,
                    to: sample_addr,
                    amount: Amount::from_raw(slot.period),
                    effective_received_amount: Amount::from_raw(slot.period),
                    op_id,
                    succeed: true,
                    fee: Amount::zero(),
                }]),
            )
        });

    api_public.0.consensus_controller = Box::new(consensus_ctrl);
    api_public.0.execution_controller = Box::new(exec_ctrl);

    let api_public_handle = api_public
        .serve(&addr, &config)
        .await
        .expect("failed to start PUBLIC API");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').next_back().unwrap()
        ))
        .unwrap();

    let response: Vec<Vec<massa_api_exports::execution::Transfer>> = client
        .request(
            "get_slots_transfers",
            rpc_params![vec![present_a, missing, present_b]],
        )
        .await
        .unwrap();

    // One entry per input slot, in order.
    assert_eq!(response.len(), 3);
    // Present slots carry their transfer; the missing slot is empty but still positioned.
    assert_eq!(response[0].len(), 1);
    assert!(response[1].is_empty());
    assert_eq!(response[2].len(), 1);
    // res[i] maps to slots[i]: the amount was set to the slot period.
    assert_eq!(response[0][0].amount, Amount::from_raw(present_a.period));
    assert_eq!(response[2][0].amount, Amount::from_raw(present_b.period));

    api_public_handle.stop().await;
}

#[tokio::test]
async fn get_stakers() {
    let addr: SocketAddr = "[::]:5015".parse().unwrap();
    let (mut api_public, config) = start_public_api(addr);

    let mut exec_ctrl = MockExecutionController::new();
    exec_ctrl.expect_get_cycle_active_rolls().returning(|_| {
        let mut map = std::collections::BTreeMap::new();
        map.insert(
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
            5_u64,
        );
        map.insert(
            Address::from_str("AU12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G").unwrap(),
            10_u64,
        );
        map.insert(
            Address::from_str("AU12cMW9zRKFDS43Z2W88VCmdQFxmHjAo54XvuVV34UzJeXRLXW9M").unwrap(),
            20_u64,
        );
        map.insert(
            Address::from_public_key(&KeyPair::generate(0).unwrap().get_public_key()),
            30_u64,
        );

        map
    });

    api_public.0.execution_controller = Box::new(exec_ctrl);

    let api_public_handle = api_public
        .serve(&addr, &config)
        .await
        .expect("failed to start PUBLIC API");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').next_back().unwrap()
        ))
        .unwrap();
    let params = rpc_params![];

    let response: Value = client.request("get_stakers", params).await.unwrap();

    response.as_array().unwrap().iter().for_each(|v| {
        let staker: (Address, u64) = serde_json::from_value(v.clone()).unwrap();
        assert!(staker.1 > 4);
    });

    api_public_handle.stop().await;
}
