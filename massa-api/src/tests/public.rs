//! Copyright (c) 2023 MASSA LABS <info@massa.net>
//!

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    str::FromStr,
};

use jsonrpsee::{
    core::{client::ClientT, Error},
    http_client::HttpClientBuilder,
    rpc_params,
};
use massa_api_exports::{
    block::{BlockInfo, BlockSummary},
    datastore::{DatastoreEntryInput, DatastoreEntryOutput},
    endorsement::EndorsementInfo,
    operation::OperationInfo,
    TimeInterval,
};
use massa_consensus_exports::{
    block_graph_export::BlockGraphExport, block_status::ExportCompiledBlock,
    test_exports::MockConsensusControllerImpl,
};

use massa_grpc::tests::mock::{MockExecutionCtrl, MockPoolCtrl};
use massa_models::{
    address::Address,
    block::{Block, BlockGraphStatus},
    clique::Clique,
    endorsement::EndorsementId,
    node::NodeId,
    operation::OperationId,
    prehash::{CapacityAllocator, PreHashMap},
    slot::Slot,
    stats::{ConsensusStats, ExecutionStats, NetworkStats},
};
use massa_protocol_exports::{
    test_exports::tools::{create_block, create_endorsement, create_operation_with_expire_period},
    MockProtocolController,
};
use massa_signature::KeyPair;
use massa_time::MassaTime;
// use serde_json::Value;

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

#[tokio::test]
async fn get_cliques() {
    let addr: SocketAddr = "[::]:5002".parse().unwrap();
    let (mut api_public, config) = start_public_api(addr).await;

    let mut consensus_ctrl = MockConsensusControllerImpl::new();
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
            addr.to_string().split(':').into_iter().last().unwrap()
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
    let (mut api_public, config) = start_public_api(addr).await;
    let keypair = KeyPair::generate(0).unwrap();
    let op = create_operation_with_expire_period(&keypair, 500000);

    api_public.0.storage.store_operations(vec![op.clone()]);

    let mut pool_ctrl = MockPoolCtrl::new();
    pool_ctrl
        .expect_contains_operations()
        .returning(|ids| ids.into_iter().map(|_id| true).collect());

    let mut exec_ctrl = MockExecutionCtrl::new();
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
            addr.to_string().split(':').into_iter().last().unwrap()
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
    let (mut api_public, config) = start_public_api(addr).await;

    let end = create_endorsement();
    api_public.0.storage.store_endorsements(vec![end.clone()]);

    let mut pool_ctrl = MockPoolCtrl::new();
    pool_ctrl
        .expect_contains_endorsements()
        .returning(|ids| ids.iter().map(|_| true).collect::<Vec<bool>>());

    let mut consensus_ctrl = MockConsensusControllerImpl::new();
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
            addr.to_string().split(':').into_iter().last().unwrap()
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
    let (mut api_public, config) = start_public_api(addr).await;
    let keypair = KeyPair::generate(0).unwrap();
    let block = create_block(&keypair);

    api_public.0.storage.store_block(block.clone());

    let mut consensus_ctrl = MockConsensusControllerImpl::new();
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
            addr.to_string().split(':').into_iter().last().unwrap()
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
    let (mut api_public, config) = start_public_api(addr).await;

    let block = create_block(&KeyPair::generate(0).unwrap());
    let id = block.id.clone();

    api_public.0.storage.store_block(block.clone());

    let mut consensus_ctrl = MockConsensusControllerImpl::new();
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
            addr.to_string().split(':').into_iter().last().unwrap()
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
    let (mut api_public, config) = start_public_api(addr).await;

    let mut consensus_ctrl = MockConsensusControllerImpl::new();
    consensus_ctrl
        .expect_get_block_graph_status()
        .returning(|_start, _end| {
            let block = create_block(&KeyPair::generate(0).unwrap());
            let id = block.id.clone();

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
            addr.to_string().split(':').into_iter().last().unwrap()
        ))
        .unwrap();

    let params = rpc_params![TimeInterval {
        start: Some(MassaTime::now().unwrap()),
        end: Some(MassaTime::now().unwrap())
    }];
    let response: Vec<BlockSummary> = client
        .request("get_graph_interval", params.clone())
        .await
        .unwrap();
    assert!(response.len() == 2);
    api_public_handle.stop().await;
}

#[tokio::test]
async fn get_datastore_entries() {
    let addr: SocketAddr = "[::]:5009".parse().unwrap();
    let (mut api_public, config) = start_public_api(addr).await;

    let mut exec_ctrl = MockExecutionCtrl::new();
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
            addr.to_string().split(':').into_iter().last().unwrap()
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

    let entry = response.get(0).unwrap();

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
    let (api_public, config) = start_public_api(addr).await;

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

// #[tokio::test]
// async fn get_stakers() {
//     let addr: SocketAddr = "[::]:5004".parse().unwrap();
//     let (mut api_public, config) = start_public_api(addr).await;

//     let mut exec_ctrl = MockExecutionCtrl::new();
//     exec_ctrl.expect_get_cycle_active_rolls().returning(|_| {
//         let mut map = std::collections::BTreeMap::new();
//         map.insert(
//             Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
//             5 as u64,
//         );
//         map.insert(
//             Address::from_str("AU12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G").unwrap(),
//             10 as u64,
//         );
//         map.insert(
//             Address::from_str("AU12cMW9zRKFDS43Z2W88VCmdQFxmHjAo54XvuVV34UzJeXRLXW9M").unwrap(),
//             20 as u64,
//         );
//         map.insert(
//             Address::from_public_key(&KeyPair::generate(0).unwrap().get_public_key()),
//             30 as u64,
//         );

//         map
//     });

//     api_public.0.execution_controller = Box::new(exec_ctrl);

//     let api_public_handle = api_public
//         .serve(&addr, &config)
//         .await
//         .expect("failed to start PUBLIC API");

//     let client = HttpClientBuilder::default()
//         .build(format!(
//             "http://localhost:{}",
//             addr.to_string().split(':').into_iter().last().unwrap()
//         ))
//         .unwrap();
//     let params = rpc_params![];

//     let response: massa_api_exports::page::PagedVec<(Address, u64)> =
//         client.request("get_stakers", params).await.unwrap();

//     // dbg!(response);

//     api_public_handle.stop().await;
// }
