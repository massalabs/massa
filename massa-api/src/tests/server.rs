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
        addr.to_string().split(':').next_back().unwrap()
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
            addr.to_string().split(':').next_back().unwrap()
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
        addr.to_string().split(':').next_back().unwrap()
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
            addr.to_string().split(':').next_back().unwrap()
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
        addr.to_string().split(':').next_back().unwrap()
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
            addr.to_string().split(':').next_back().unwrap()
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
            addr2.to_string().split(':').next_back().unwrap()
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

#[cfg(test)]
mod jsonrpc_error_classification {
    use super::*;
    use massa_execution_exports::{ExecutionQueryResponse, MockExecutionController};
    use massa_models::slot::Slot;
    use serde_json::{json, Value};
    use std::{
        io::{Read, Write},
        net::TcpStream,
        sync::{Arc, Mutex},
        time::Duration,
    };

    const ENTRIES: &str = "get_datastore_entries";
    type Counts = Arc<Mutex<[usize; 2]>>;

    fn request(method: &str, id: Option<Value>) -> Value {
        let mut value = json!({"jsonrpc":"2.0","method":method,"params":[[]]});
        if let Some(id) = id {
            value["id"] = id;
        }
        value
    }

    async fn server(port: u16, limit: u32) -> (crate::StopHandle, Counts) {
        let address = SocketAddr::from(([127, 0, 0, 1], port));
        let (mut api, mut config) = crate::tests::mock::start_public_api(address);
        config.batch_request_limit = limit;
        config.max_request_body_size = 16 * 1024;
        config.max_response_body_size = 16 * 1024;
        config.max_connections = 8;
        config.enable_ws = false;
        api.0.api_settings = config.clone();
        let counts = Arc::new(Mutex::new([0, 0]));
        let mut backend = MockExecutionController::new();
        let entry_counts = counts.clone();
        backend
            .expect_get_final_and_active_data_entry()
            .returning(move |input| {
                assert!(input.is_empty());
                entry_counts.lock().unwrap()[0] += 1;
                Vec::new()
            });
        let key_counts = counts.clone();
        backend.expect_query_state().returning(move |input| {
            assert!(input.requests.is_empty());
            key_counts.lock().unwrap()[1] += 1;
            ExecutionQueryResponse {
                responses: Vec::new(),
                candidate_cursor: Slot::new(2, 1),
                final_cursor: Slot::new(1, 0),
                final_state_fingerprint: massa_hash::Hash::compute_from(b"protocol-pilot"),
            }
        });
        api.0.execution_controller = Box::new(backend);
        (api.serve(&address, &config).await.unwrap(), counts)
    }

    fn parse_http(bytes: &[u8]) -> (u16, String) {
        assert!(bytes.len() <= 64 * 1024);
        let text = std::str::from_utf8(bytes).unwrap();
        let (headers, body) = text.split_once("\r\n\r\n").expect("complete HTTP headers");
        let mut lines = headers.split("\r\n");
        let mut status = lines.next().unwrap().split_whitespace();
        assert_eq!(status.next(), Some("HTTP/1.1"));
        let code = status.next().unwrap().parse().unwrap();
        let mut length = None;
        for line in lines {
            let (name, value) = line.split_once(':').unwrap();
            assert!(
                !name.eq_ignore_ascii_case("transfer-encoding"),
                "unexpected transfer coding"
            );
            if name.eq_ignore_ascii_case("content-length") {
                assert!(length.is_none(), "duplicate HTTP length");
                length = Some(value.trim().parse::<usize>().unwrap());
            }
        }
        assert_eq!(length, Some(body.len()), "complete HTTP body");
        (code, body.to_string())
    }

    async fn post(port: u16, body: String) -> (u16, String) {
        assert!(body.len() <= 16 * 1024);
        tokio::task::spawn_blocking(move || {
            let address=SocketAddr::from(([127,0,0,1],port));
            let mut stream=TcpStream::connect_timeout(&address,Duration::from_secs(2)).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
            write!(stream,"POST / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
            let mut bytes=Vec::new();stream.take(64*1024+1).read_to_end(&mut bytes).unwrap();
            parse_http(&bytes)
        }).await.unwrap()
    }

    fn count(counts: &Counts) -> [usize; 2] {
        *counts.lock().unwrap()
    }
    fn delta(before: [usize; 2], after: [usize; 2]) -> [usize; 2] {
        [after[0] - before[0], after[1] - before[1]]
    }
    async fn recovery(port: u16, counts: &Counts, name: &str) -> Value {
        let id = format!("recover-{name}");
        let before = count(counts);
        let (status, body) = post(port, request(ENTRIES, Some(json!(id))).to_string()).await;
        assert_eq!(status, 200, "recovery HTTP status");
        let value: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            value,
            json!({"jsonrpc":"2.0","id":id,"result":[]}),
            "recovery exact response"
        );
        let calls = delta(before, count(counts));
        assert_eq!(calls, [1, 0], "recovery dispatch");
        json!({"status":status,"body":body,"dispatch":calls})
    }

    // Shared transport fixtures; each error must preserve a usable server.
    fn error_cases() -> Vec<(&'static str, i32, Value)> {
        vec![
            (
                r#"{"jsonrpc":"1.0","method":"get_datastore_entries","params":[[]]}"#,
                -32600,
                Value::Null,
            ),
            (r#"{"jsonrpc":"2.0"}"#, -32600, Value::Null),
            (r#"{}"#, -32600, Value::Null),
            (
                r#"{"jsonrpc":"1.0","id":77,"method":"get_datastore_entries","params":[[]]}"#,
                -32600,
                json!(77),
            ),
            (
                r#"{"jsonrpc":"2.0","id":"keep-id"}"#,
                -32600,
                json!("keep-id"),
            ),
            (
                r#"{"jsonrpc":"2.0","method":"get_datastore_entries""#,
                -32700,
                Value::Null,
            ),
            (r#"{"id":12,"method":}"#, -32700, Value::Null),
            (r#"{"jsonrpc":"2.0"} trailing"#, -32700, Value::Null),
        ]
    }

    fn assert_error(raw: &str, code: i32, id: Value) {
        let value: Value = serde_json::from_str(raw).unwrap();
        assert_eq!(value["jsonrpc"], "2.0");
        assert_eq!(value["id"], id);
        assert_eq!(value["error"]["code"], code);
        assert!(value["error"]["message"].is_string());
        assert!(value.get("result").is_none());
    }

    #[tokio::test]
    async fn invalid_envelope_rejection_http_error_classes() {
        let port = 46111;
        let (handle, counts) = server(port, 16).await;
        for (index, (raw, code, id)) in error_cases().into_iter().enumerate() {
            let before = count(&counts);
            let (status, body) = post(port, raw.to_string()).await;
            assert_eq!(status, 200);
            assert_error(&body, code, id);
            assert_eq!(count(&counts), before, "invalid envelope dispatched");
            recovery(port, &counts, &index.to_string()).await;
        }
        for id in [json!(0), json!("valid"), Value::Null] {
            let before = count(&counts);
            let (status, body) = post(port, request(ENTRIES, Some(id.clone())).to_string()).await;
            assert_eq!(status, 200);
            assert_eq!(
                serde_json::from_str::<Value>(&body).unwrap(),
                json!({"jsonrpc":"2.0","id":id,"result":[]})
            );
            assert_eq!(delta(before, count(&counts)), [1, 0]);
        }
        handle.stop().await;
    }

    #[tokio::test]
    async fn invalid_envelope_rejection_websocket_error_classes() {
        use jsonrpsee::client_transport::ws::WsTransportClientBuilder;
        use jsonrpsee::core::client::{ReceivedMessage, TransportReceiverT, TransportSenderT};
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let mut module = jsonrpsee::RpcModule::new(calls.clone());
        module
            .register_method("tick", |_, count, _| count.fetch_add(1, Ordering::SeqCst))
            .unwrap();
        let address = SocketAddr::from(([127, 0, 0, 1], 46112));
        let (_, mut config) = crate::tests::mock::start_public_api(address);
        config.enable_http = true;
        config.enable_ws = true;
        let handle = crate::serve(module, &address, &config).await.unwrap();
        let (mut sender, mut receiver) = WsTransportClientBuilder::default()
            .build(Url::parse("ws://127.0.0.1:46112").unwrap())
            .await
            .unwrap();
        for (index, (raw, code, id)) in error_cases().into_iter().enumerate() {
            sender.send(raw.to_string()).await.unwrap();
            let reply = tokio::time::timeout(Duration::from_secs(2), receiver.receive())
                .await
                .unwrap()
                .unwrap();
            let ReceivedMessage::Text(raw) = reply else {
                panic!("unexpected WS frame")
            };
            assert_error(&raw, code, id);
            assert_eq!(calls.load(Ordering::SeqCst), index);
            sender
                .send(json!({"jsonrpc":"2.0","id":index,"method":"tick"}).to_string())
                .await
                .unwrap();
            let reply = tokio::time::timeout(Duration::from_secs(2), receiver.receive())
                .await
                .unwrap()
                .unwrap();
            let ReceivedMessage::Text(raw) = reply else {
                panic!("unexpected WS frame")
            };
            assert_eq!(
                serde_json::from_str::<Value>(&raw).unwrap(),
                json!({"jsonrpc":"2.0","id":index,"result":index})
            );
            assert_eq!(calls.load(Ordering::SeqCst), index + 1);
        }
        drop(sender);
        drop(receiver);
        handle.stop().await;
    }
}
