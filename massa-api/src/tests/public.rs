//! Copyright (c) 2023 MASSA LABS <info@massa.net>
//!

use std::{collections::HashMap, net::SocketAddr};

use jsonrpsee::{core::client::ClientT, http_client::HttpClientBuilder, rpc_params};
use massa_consensus_exports::test_exports::MockConsensusControllerImpl;

use massa_grpc::tests::mock::{MockExecutionCtrl, MockPoolCtrl};
use massa_models::{
    slot::Slot,
    stats::{ConsensusStats, ExecutionStats, NetworkStats},
};
use massa_protocol_exports::MockProtocolController;
use massa_time::MassaTime;

use crate::{tests::mock::start_public_api, RpcServer};

#[tokio::test]
async fn get_status() {
    let addr: SocketAddr = "[::]:5001".parse().unwrap();
    let (mut api_public, config) = start_public_api(addr).await;

    let mut exec_ctrl = MockExecutionCtrl::new();

    exec_ctrl.expect_get_stats().returning(|| ExecutionStats {
        time_window_start: MassaTime::now().unwrap(),
        time_window_end: MassaTime::now().unwrap(),
        final_block_count: 0,
        final_executed_operations_count: 0,
        active_cursor: Slot::new(0, 0),
        final_cursor: Slot::new(0, 0),
    });

    let mut consensus_ctrl = MockConsensusControllerImpl::new();
    consensus_ctrl.expect_get_stats().returning(|| {
        Ok(ConsensusStats {
            start_timespan: MassaTime::now().unwrap(),
            end_timespan: MassaTime::now().unwrap(),
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

    let mut pool_ctrl = MockPoolCtrl::new();
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
            addr.to_string().split(':').into_iter().last().unwrap()
        ))
        .unwrap();
    let params = rpc_params![];
    let response: massa_api_exports::node::NodeStatus =
        client.request("get_status", params).await.unwrap();

    assert_eq!(response.network_stats.in_connection_count, 10);
    assert_eq!(response.network_stats.out_connection_count, 5);
    assert_eq!(response.config.thread_count, 32);

    api_public_handle.stop().await;
}
