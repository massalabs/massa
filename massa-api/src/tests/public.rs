//! Copyright (c) 2023 MASSA LABS <info@massa.net>
//!

use std::{
    collections::{BTreeMap, HashMap},
    net::{IpAddr, SocketAddr},
    str::FromStr,
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

use crate::{tests::mock::start_public_api, RpcServer};
use massa_execution_exports::{
    ExecutionAddressInfo, ExecutionQueryResponse, ExecutionQueryResponseItem,
    MockExecutionController, ReadOnlyExecutionOutput,
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
    pool_ctrl.expect_get_operation_count().returning(|| 1024);
    pool_ctrl.expect_get_endorsement_count().returning(|| 2048);

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
            addr.to_string().split(':').last().unwrap()
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
            addr.to_string().split(':').last().unwrap()
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
        .returning(|ids| ids.iter().map(|_id| true).collect());

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
            addr.to_string().split(':').last().unwrap()
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
        .returning(|ids| ids.iter().map(|_| true).collect::<Vec<bool>>());

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
            addr.to_string().split(':').last().unwrap()
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
            addr.to_string().split(':').last().unwrap()
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
            addr.to_string().split(':').last().unwrap()
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
            addr.to_string().split(':').last().unwrap()
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
        pool_ctrl.expect_add_operations().returning(|_a| ());
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
            addr.to_string().split(':').last().unwrap()
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
        pool_ctrl.expect_add_operations().returning(|_a| ());
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
            addr.to_string().split(':').last().unwrap()
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
            addr.to_string().split(':').last().unwrap()
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
            addr.to_string().split(':').last().unwrap()
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
            addr.to_string().split(':').last().unwrap()
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
    exec_ctrl.expect_get_addresses_infos().returning(|a, _s| {
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
            addr.to_string().split(':').last().unwrap()
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
            addr.to_string().split(':').last().unwrap()
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
            addr.to_string().split(':').last().unwrap()
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
            addr.to_string().split(':').last().unwrap()
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
            addr.to_string().split(':').last().unwrap()
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
            addr.to_string().split(':').last().unwrap()
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
            addr.to_string().split(':').last().unwrap()
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
