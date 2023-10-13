use std::{collections::BTreeMap, net::SocketAddr, str::FromStr, time::Duration};

use jsonrpsee::{
    async_client::ClientBuilder,
    client_transport::ws::{Url, WsTransportClientBuilder},
    core::client::{ClientT, Subscription, SubscriptionClientT},
    rpc_params,
    ws_client::WsClientBuilder,
};
use massa_consensus_exports::test_exports::MockConsensusControllerImpl;
use massa_grpc::tests::mock::MockExecutionCtrl;
use massa_models::{
    address::Address,
    block::{FilledBlock, SecureShareBlock},
    block_header::BlockHeader,
    block_id::BlockId,
    config::VERSION,
    secure_share::SecureShare,
};
use massa_protocol_exports::test_exports::tools::create_block;
use massa_signature::KeyPair;
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

#[tokio::test]
async fn subscribe_new_blocks() {
    let addr: SocketAddr = "[::]:5033".parse().unwrap();
    let (mut api_server, api_config) = get_apiv2_server(&addr);

    let (tx, _rx) = tokio::sync::broadcast::channel::<SecureShareBlock>(10);

    api_server.0.consensus_channels.block_sender = tx.clone();

    let block = create_block(&KeyPair::generate(0).unwrap());

    let api_handle = api_server
        .serve(&addr, &api_config)
        .await
        .expect("failed to start MASSA API V2");

    let uri = Url::parse(&format!(
        "ws://localhost:{}",
        addr.to_string().split(':').into_iter().last().unwrap()
    ))
    .unwrap();

    let client1 = WsClientBuilder::default().build(&uri).await.unwrap();
    let mut sub1: Subscription<Value> = client1
        .subscribe("subscribe_new_blocks", rpc_params![], "unsubscribe_hello")
        .await
        .unwrap();

    let to_send = block.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let _ = tx.send(to_send).unwrap();
    });

    let result = tokio::time::timeout(Duration::from_secs(4), sub1.next())
        .await
        .unwrap();

    assert!(result.is_some());
    let value = result.unwrap().unwrap().clone();
    assert_eq!(value["id"].as_str().unwrap(), &block.id.to_string());
    api_handle.stop().await;
}

#[tokio::test]
async fn subscribe_new_blocks_headers() {
    let addr: SocketAddr = "[::]:5034".parse().unwrap();
    let (mut api_server, api_config) = get_apiv2_server(&addr);

    let uri = Url::parse(&format!(
        "ws://localhost:{}",
        addr.to_string().split(':').into_iter().last().unwrap()
    ))
    .unwrap();
    let (tx, _rx) = tokio::sync::broadcast::channel::<SecureShare<BlockHeader, BlockId>>(10);

    api_server.0.consensus_channels.block_header_sender = tx.clone();

    let api_handle = api_server
        .serve(&addr, &api_config)
        .await
        .expect("failed to start MASSA API V2");
    let block = create_block(&KeyPair::generate(0).unwrap());

    let client1 = WsClientBuilder::default().build(&uri).await.unwrap();
    let mut sub1: Subscription<Value> = client1
        .subscribe(
            "subscribe_new_blocks_headers",
            rpc_params![],
            "unsubscribe_hello",
        )
        .await
        .unwrap();

    let to_send = block.content.header.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let _ = tx.send(to_send).unwrap();
    });

    let result = tokio::time::timeout(Duration::from_secs(4), sub1.next())
        .await
        .unwrap();

    assert!(result.is_some());
    let value = result.unwrap().unwrap().clone();
    assert_eq!(value["id"].as_str().unwrap(), &block.id.to_string());

    api_handle.stop().await;
}

#[tokio::test]
async fn subscribe_new_filled_blocks() {
    let addr: SocketAddr = "[::]:5035".parse().unwrap();
    let (mut api_server, api_config) = get_apiv2_server(&addr);

    let uri = Url::parse(&format!(
        "ws://localhost:{}",
        addr.to_string().split(':').into_iter().last().unwrap()
    ))
    .unwrap();
    let (tx, _rx) = tokio::sync::broadcast::channel::<FilledBlock>(10);

    api_server.0.consensus_channels.filled_block_sender = tx.clone();

    let api_handle = api_server
        .serve(&addr, &api_config)
        .await
        .expect("failed to start MASSA API V2");
    let block = create_block(&KeyPair::generate(0).unwrap());

    let client1 = WsClientBuilder::default().build(&uri).await.unwrap();
    let mut sub1: Subscription<Value> = client1
        .subscribe(
            "subscribe_new_filled_blocks",
            rpc_params![],
            "unsubscribe_hello",
        )
        .await
        .unwrap();

    let filled = FilledBlock {
        header: block.content.header,
        operations: vec![],
    };

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let _ = tx.send(filled).unwrap();
    });

    let result = tokio::time::timeout(Duration::from_secs(4), sub1.next())
        .await
        .unwrap();

    assert!(result.is_some());
    let value = result.unwrap().unwrap().clone();
    assert_eq!(
        value["header"]["id"].as_str().unwrap(),
        &block.id.to_string()
    );

    api_handle.stop().await;
}
