use std::{collections::BTreeMap, net::SocketAddr, str::FromStr};

use jsonrpsee::{
    async_client::ClientBuilder,
    client_transport::ws::{Url, WsTransportClientBuilder},
    core::client::ClientT,
    rpc_params,
};
use massa_consensus_exports::test_exports::MockConsensusControllerImpl;
use massa_grpc::tests::mock::MockExecutionCtrl;
use massa_models::{address::Address, block_id::BlockId, config::VERSION};
use serde_json::Value;

use crate::{tests::mock::get_apiv2_server, ApiServer};

#[tokio::test]
async fn get_version() {
    let addr: SocketAddr = "[::]:5030".parse().unwrap();
    let (api_server, api_config) = get_apiv2_server(&addr);

    let api_handle = api_server
        .serve(&addr, &api_config)
        .await
        .expect("failed to start MASSA API V2");

    let uri = Url::parse(&format!(
        "ws://localhost:{}",
        addr.to_string().split(':').into_iter().last().unwrap()
    ))
    .unwrap();

    let (tx, rx) = WsTransportClientBuilder::default()
        .build(uri)
        .await
        .unwrap();
    let client = ClientBuilder::default().build_with_tokio(tx, rx);
    let response: String = client.request("get_version", rpc_params![]).await.unwrap();

    assert_eq!(response, *VERSION.to_string());

    api_handle.stop().await;
}

#[tokio::test]
async fn get_next_block_best_parents() {
    let addr: SocketAddr = "[::]:5031".parse().unwrap();
    let (mut api_server, api_config) = get_apiv2_server(&addr);

    let mut consensus_ctrl = MockConsensusControllerImpl::new();
    consensus_ctrl.expect_get_best_parents().returning(|| {
        vec![(
            massa_models::block_id::BlockId::from_str(
                "B12oYMQEAX35HPeDVgGdW2fYRtDs4UJTpeXqW75QPYCdEdPUZ9oV",
            )
            .unwrap(),
            100,
        )]
    });

    api_server.0.consensus_controller = Box::new(consensus_ctrl);

    let api_handle = api_server
        .serve(&addr, &api_config)
        .await
        .expect("failed to start MASSA API V2");

    let uri = Url::parse(&format!(
        "ws://localhost:{}",
        addr.to_string().split(':').into_iter().last().unwrap()
    ))
    .unwrap();

    let (tx, rx) = WsTransportClientBuilder::default()
        .build(uri)
        .await
        .unwrap();
    let client = ClientBuilder::default().build_with_tokio(tx, rx);
    let response: Vec<(BlockId, u64)> = client
        .request("get_next_block_best_parents", rpc_params![])
        .await
        .unwrap();

    assert_eq!(response.len(), 1);
    assert_eq!(response[0].1, 100);

    api_handle.stop().await;
}

#[tokio::test]
async fn get_largest_stakers() {
    let addr: SocketAddr = "[::]:5032".parse().unwrap();
    let (mut api_server, api_config) = get_apiv2_server(&addr);

    let mut exec_ctrl = MockExecutionCtrl::new();
    exec_ctrl.expect_get_cycle_active_rolls().returning(|_| {
        let mut map = BTreeMap::new();
        map.insert(
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
            100,
        );
        map
    });

    api_server.0.execution_controller = Box::new(exec_ctrl);

    let api_handle = api_server
        .serve(&addr, &api_config)
        .await
        .expect("failed to start MASSA API V2");

    let uri = Url::parse(&format!(
        "ws://localhost:{}",
        addr.to_string().split(':').into_iter().last().unwrap()
    ))
    .unwrap();

    let (tx, rx) = WsTransportClientBuilder::default()
        .build(uri)
        .await
        .unwrap();
    let client = ClientBuilder::default().build_with_tokio(tx, rx);
    let response: Value = client
        .request("get_largest_stakers", rpc_params![])
        .await
        .unwrap();

    assert_eq!(response["total_count"], 1);

    api_handle.stop().await;
}
