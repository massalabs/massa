use std::{net::SocketAddr, str::FromStr};

use jsonrpsee::{
    client_transport::ws::Url, core::client::ClientT, http_client::HttpClientBuilder, rpc_params,
    ws_client::WsClientBuilder,
};
use massa_api_exports::operation::OperationInfo;
use massa_models::operation::OperationId;

use crate::{tests::mock::get_apiv2_server, ApiServer, RpcServer};

#[tokio::test]
async fn max_conn() {
    let addr: SocketAddr = "[::]:5037".parse().unwrap();
    let (mut api_server, mut api_config) = get_apiv2_server(&addr);

    api_server.0.api_settings.max_connections = 10;
    api_config.max_connections = 10;

    let uri = Url::parse(&format!(
        "ws://localhost:{}",
        addr.to_string().split(':').last().unwrap()
    ))
    .unwrap();

    let api_handle = api_server
        .serve(&addr, &api_config)
        .await
        .expect("failed to start MASSA API V2");

    let mut clients = vec![];
    for _ in 0..15 {
        let client1 = WsClientBuilder::default().build(&uri).await;
        if clients.len() < 10 {
            assert!(client1.is_ok());
            clients.push(client1.unwrap());
        } else {
            assert!(client1.is_err());
        }
    }

    api_handle.stop().await;
}

#[tokio::test]
async fn max_request_size() {
    let addr: SocketAddr = "[::]:5038".parse().unwrap();
    let (mut api_server, mut api_config) = crate::tests::mock::start_public_api(addr);

    api_config.max_request_body_size = 10;
    api_server.0.api_settings.max_request_body_size = 10;

    let api_handle = api_server
        .serve(&addr, &api_config)
        .await
        .expect("failed to start MASSA API V2");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').last().unwrap()
        ))
        .unwrap();
    let params = rpc_params![vec![
        OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap(),
        OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap(),
        OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap(),
        OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap(),
        OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap(),
        OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap(),
        OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap(),
        OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap(),
        OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap(),
        OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap(),
        OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap(),
        OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap(),
        OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap(),
        OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap(),
        OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap(),
        OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap(),
        OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap(),
        OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap(),
        OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap(),
        OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap(),
    ]];
    let response: Result<Vec<OperationInfo>, jsonrpsee::core::client::Error> =
        client.request("get_operations", params).await;

    let response_str = response.unwrap_err().to_string();
    assert!(response_str.contains("Request rejected `413`"));

    api_handle.stop().await;
}

#[tokio::test]
async fn ws_disabled() {
    let addr: SocketAddr = "[::]:5039".parse().unwrap();
    let (mut api_server, mut api_config) = get_apiv2_server(&addr);

    api_server.0.api_settings.enable_ws = false;
    api_config.enable_ws = false;

    let uri = Url::parse(&format!(
        "ws://localhost:{}",
        addr.to_string().split(':').last().unwrap()
    ))
    .unwrap();

    let api_handle = api_server
        .serve(&addr, &api_config)
        .await
        .expect("failed to start MASSA API V2");
    let response = WsClientBuilder::default().build(&uri).await;
    let err = response.unwrap_err();
    assert!(
        err.to_string().contains("status code: 403")
            || err.to_string().contains("(os error 10061)")
    );

    api_handle.stop().await;
}

#[tokio::test]
async fn http_disabled() {
    let addr: SocketAddr = "[::]:5040".parse().unwrap();
    let (mut api_server, mut api_config) = crate::tests::mock::start_public_api(addr);

    api_server.0.api_settings.enable_http = false;
    api_config.enable_http = false;

    let api_handle = api_server
        .serve(&addr, &api_config)
        .await
        .expect("failed to start MASSA API V2");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').last().unwrap()
        ))
        .unwrap();

    let response: Result<Vec<OperationInfo>, jsonrpsee::core::client::Error> =
        client.request("get_operations", rpc_params![]).await;

    let response_str = response.unwrap_err().to_string();
    assert!(response_str.contains("Request rejected `403`"));

    api_handle.stop().await;
}

#[tokio::test]
async fn host_allowed() {
    let addr: SocketAddr = "[::]:5041".parse().unwrap();
    let (mut api_server, mut api_config) = crate::tests::mock::start_public_api(addr);

    let hosts = vec![format!(
        "http://localhost:{}",
        addr.to_string().split(':').last().unwrap()
    )];

    api_server.0.api_settings.allow_hosts = hosts.clone();
    api_config.allow_hosts = hosts;

    let api_handle = api_server
        .serve(&addr, &api_config)
        .await
        .expect("failed to start MASSA API V2");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr.to_string().split(':').last().unwrap()
        ))
        .unwrap();

    let response: Result<Vec<OperationInfo>, jsonrpsee::core::client::Error> =
        client.request("get_operations", rpc_params![]).await;

    // response OK but invalid params (no params provided)
    assert!(response.unwrap_err().to_string().contains("Invalid params"));

    // now start new server with different host allowed
    let addr2: SocketAddr = "[::]:5042".parse().unwrap();
    let (mut api_server2, mut api_config2) = crate::tests::mock::start_public_api(addr2);

    let hosts2 = vec!["http://123.456.789.1".to_string()];

    api_server2.0.api_settings.allow_hosts = hosts2.clone();
    api_config2.allow_hosts = hosts2;

    let api_handle2 = api_server2
        .serve(&addr2, &api_config2)
        .await
        .expect("failed to start MASSA API V2");

    let client = HttpClientBuilder::default()
        .build(format!(
            "http://localhost:{}",
            addr2.to_string().split(':').last().unwrap()
        ))
        .unwrap();

    let response: Result<Vec<OperationInfo>, jsonrpsee::core::client::Error> =
        client.request("get_operations", rpc_params![]).await;

    // host not allowed
    let response_str = response.unwrap_err().to_string();
    assert!(response_str.contains("Request rejected `403`"));

    api_handle.stop().await;
    api_handle2.stop().await;
}
