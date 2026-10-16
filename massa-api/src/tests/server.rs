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
    const KEYS: &str = "get_addresses_datastore_keys";
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

    #[tokio::test]
    async fn invalid_envelope_rejection_http_invalid_ids() {
        let port = 46113;
        let (handle, counts) = server(port, 16).await;
        for (index, id) in [json!(true), json!(false), json!({}), json!([])]
            .into_iter()
            .enumerate()
        {
            let invalid = request(ENTRIES, Some(id));
            let before = count(&counts);
            let (status, body) = post(port, invalid.to_string()).await;
            assert_eq!(status, 200);
            assert!(!body.is_empty(), "invalid_id_error_missing");
            assert_error(&body, -32600, Value::Null);
            assert_eq!(
                delta(before, count(&counts)),
                [0, 0],
                "invalid ID dispatched"
            );
            recovery(port, &counts, &format!("single-{index}")).await;

            let before = count(&counts);
            let good = request("get_addresses_datastore_keys", Some(json!("okay")));
            let null_call = request(ENTRIES, Some(Value::Null));
            let (status, body) = post(port, json!([invalid, good, null_call]).to_string()).await;
            assert_eq!(status, 200);
            let reply: Value = serde_json::from_str(&body).unwrap();
            let replies = reply.as_array().unwrap();
            assert_eq!(replies.len(), 3, "invalid batch member omitted");
            assert_error(&replies[0].to_string(), -32600, Value::Null);
            assert_eq!(replies[1], json!({"jsonrpc":"2.0","id":"okay","result":[]}));
            assert_eq!(replies[2], json!({"jsonrpc":"2.0","id":null,"result":[]}));
            assert_eq!(
                delta(before, count(&counts)),
                [1, 1],
                "mixed batch dispatch"
            );
            recovery(port, &counts, &format!("batch-{index}")).await;
        }

        // ID omission and unknown members remain allowed notification syntax.
        let mut notification = request(ENTRIES, None);
        notification["extra"] = json!({"allowed":true});
        let (status, body) = post(port, notification.to_string()).await;
        assert_eq!(status, 200);
        assert!(body.is_empty());
        let (status, body) = post(
            port,
            json!([notification, request(ENTRIES, Some(json!("peer")))]).to_string(),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap(),
            json!([{"jsonrpc":"2.0","id":"peer","result":[]}])
        );
        recovery(port, &counts, "omitted-id").await;
        handle.stop().await;
    }

    #[tokio::test]
    async fn invalid_envelope_rejection_websocket_invalid_ids() {
        use jsonrpsee::client_transport::ws::WsTransportClientBuilder;
        use jsonrpsee::core::client::{ReceivedMessage, TransportReceiverT, TransportSenderT};
        use std::sync::atomic::{AtomicUsize, Ordering};

        async fn receive(receiver: &mut impl TransportReceiverT) -> Value {
            let reply = tokio::time::timeout(Duration::from_secs(2), receiver.receive())
                .await
                .unwrap()
                .unwrap();
            let ReceivedMessage::Text(raw) = reply else {
                panic!("unexpected WS frame")
            };
            serde_json::from_str(&raw).unwrap()
        }

        let calls = Arc::new(AtomicUsize::new(0));
        let mut module = jsonrpsee::RpcModule::new(calls.clone());
        module
            .register_method("tick", |_, count, _| count.fetch_add(1, Ordering::SeqCst))
            .unwrap();
        let address = SocketAddr::from(([127, 0, 0, 1], 46114));
        let (_, mut config) = crate::tests::mock::start_public_api(address);
        config.enable_http = true;
        config.enable_ws = true;
        let handle = crate::serve(module, &address, &config).await.unwrap();
        let (mut sender, mut receiver) = WsTransportClientBuilder::default()
            .build(Url::parse("ws://127.0.0.1:46114").unwrap())
            .await
            .unwrap();

        for (index, id) in [json!(true), json!(false), json!({}), json!([])]
            .into_iter()
            .enumerate()
        {
            let invalid = request("tick", Some(id));
            let before = calls.load(Ordering::SeqCst);
            sender.send(invalid.to_string()).await.unwrap();
            assert_error(
                &receive(&mut receiver).await.to_string(),
                -32600,
                Value::Null,
            );
            assert_eq!(
                calls.load(Ordering::SeqCst),
                before,
                "invalid ID dispatched"
            );
            let recovery_id = format!("single-{index}");
            sender
                .send(request("tick", Some(json!(recovery_id))).to_string())
                .await
                .unwrap();
            assert_eq!(
                receive(&mut receiver).await,
                json!({"jsonrpc":"2.0","id":recovery_id,"result":before})
            );
            assert_eq!(calls.load(Ordering::SeqCst), before + 1);

            let before = calls.load(Ordering::SeqCst);
            let good = request("tick", Some(json!("okay")));
            let null_call = request("tick", Some(Value::Null));
            sender
                .send(json!([invalid, good, null_call]).to_string())
                .await
                .unwrap();
            let reply = receive(&mut receiver).await;
            let replies = reply.as_array().unwrap();
            assert_eq!(replies.len(), 3, "invalid batch member omitted");
            assert_error(&replies[0].to_string(), -32600, Value::Null);
            assert_eq!(
                replies[1],
                json!({"jsonrpc":"2.0","id":"okay","result":before})
            );
            assert_eq!(
                replies[2],
                json!({"jsonrpc":"2.0","id":null,"result":before + 1})
            );
            assert_eq!(
                calls.load(Ordering::SeqCst),
                before + 2,
                "mixed batch dispatch"
            );
            let recovery_id = format!("batch-{index}");
            sender
                .send(request("tick", Some(json!(recovery_id))).to_string())
                .await
                .unwrap();
            assert_eq!(
                receive(&mut receiver).await,
                json!({"jsonrpc":"2.0","id":recovery_id,"result":before + 2})
            );
            assert_eq!(calls.load(Ordering::SeqCst), before + 3);
        }

        for id in [json!(0), json!("valid"), Value::Null] {
            let before = calls.load(Ordering::SeqCst);
            sender
                .send(request("tick", Some(id.clone())).to_string())
                .await
                .unwrap();
            assert_eq!(
                receive(&mut receiver).await,
                json!({"jsonrpc":"2.0","id":id,"result":before})
            );
            assert_eq!(calls.load(Ordering::SeqCst), before + 1);
        }
        let mut notification = request("tick", None);
        notification["extra"] = json!({"allowed":true});
        sender.send(notification.to_string()).await.unwrap();
        sender
            .send(request("tick", Some(json!("after-notification"))).to_string())
            .await
            .unwrap();
        let reply = receive(&mut receiver).await;
        assert_eq!(reply["id"], "after-notification");
        assert!(reply.get("result").is_some());
        assert!(reply.get("error").is_none());
        drop(sender);
        drop(receiver);
        handle.stop().await;
    }

    #[tokio::test]
    async fn notification_dispatch_ordinary_calls_and_notifications() {
        let port = 46101;
        let (handle, counts) = server(port, 16).await;
        let mut nested_id = request(ENTRIES, None);
        nested_id["extra"] = json!({"id":true});
        let mut bad_params = request(KEYS, None);
        bad_params["params"] = json!([]);
        let cases = [
            (request(ENTRIES, None), None, [1, 0]),
            (request(KEYS, None), None, [0, 1]),
            (nested_id, None, [1, 0]),
            (
                json!([request(ENTRIES, None), request(KEYS, None)]),
                None,
                [1, 1],
            ),
            (
                json!([
                    request(ENTRIES, Some(json!(1))),
                    request(KEYS, None),
                    request(KEYS, Some(json!("keys"))),
                    request(ENTRIES, None)
                ]),
                Some(json!([{"jsonrpc":"2.0","id":1,"result":[]},
                    {"jsonrpc":"2.0","id":"keys","result":[]}])),
                [2, 2],
            ),
            (request("unknown_fixture", None), None, [0, 0]),
            (bad_params, None, [0, 0]),
            (
                json!([
                    ["2.0", "get_datastore_entries", [[]]],
                    request(KEYS, Some(json!(9)))
                ]),
                Some(
                    json!([{"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid request"}},
                    {"jsonrpc":"2.0","id":9,"result":[]}]),
                ),
                [0, 1],
            ),
        ];
        for (index, (raw, expected, wanted)) in cases.into_iter().enumerate() {
            let before = count(&counts);
            let (status, body) = post(port, raw.to_string()).await;
            assert_eq!(status, 200);
            match expected {
                Some(value) => assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), value),
                None => assert!(body.is_empty(), "notification emitted a response"),
            }
            assert_eq!(
                delta(before, count(&counts)),
                wanted,
                "notification_not_dispatched"
            );
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
    async fn configured_resource_limits_limit_boundaries_and_recovery() {
        let (handle, counts) = server(46103, 16).await;
        let mut input = vec![request(ENTRIES, None); 15];
        input.push(request(KEYS, Some(json!(16))));
        let (status, body) = post(46103, Value::Array(input).to_string()).await;
        assert_eq!(status, 200);
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap(),
            json!([{"jsonrpc":"2.0","id":16,"result":[]}])
        );
        assert_eq!(count(&counts), [15, 1]);
        let before = count(&counts);
        let mut input = vec![request(ENTRIES, None); 16];
        input.push(request(KEYS, Some(json!(17))));
        let (status, body) = post(46103, Value::Array(input).to_string()).await;
        assert_eq!(status, 200);
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap(),
            json!({"jsonrpc":"2.0","id":null,"error":{"code":-32010,"message":"The batch request was too large","data":"Exceeded max limit of 16"}}),
            "guard_batch_limit"
        );
        assert_eq!(
            delta(before, count(&counts)),
            [0, 0],
            "guard_before_dispatch"
        );
        recovery(46103, &counts, "limit").await;
        handle.stop().await;
        println!(
            "PROTOCOL_GUARD {}",
            json!({"case":"limit","accepted":16,"rejected":17,"pre_dispatch":true,"recovery":true})
        );
    }

    #[tokio::test]
    async fn protocol_batches_disabled() {
        let (handle, counts) = server(46104, 0).await;
        let (status, body) =
            post(46104, json!([request(ENTRIES, Some(json!(1)))]).to_string()).await;
        assert_eq!(status, 200);
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap(),
            json!({"jsonrpc":"2.0","id":null,"error":{"code":-32005,"message":"Batched requests are not supported by this server"}}),
            "guard_batches_disabled"
        );
        assert_eq!(count(&counts), [0, 0], "guard_disabled_before_dispatch");
        recovery(46104, &counts, "disabled").await;
        handle.stop().await;
        println!(
            "PROTOCOL_GUARD {}",
            json!({"case":"disabled","pre_dispatch":true,"recovery":true})
        );
    }

    #[tokio::test]
    async fn body_and_result_byte_limits() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let mut module = jsonrpsee::RpcModule::new(calls.clone());
        module
            .register_method("big", |_, ctx, _| {
                ctx.fetch_add(1, Ordering::SeqCst);
                "x".repeat(4096)
            })
            .unwrap();
        module
            .register_method("small", |_, ctx, _| {
                ctx.fetch_add(1, Ordering::SeqCst);
                7
            })
            .unwrap();
        let address = SocketAddr::from(([127, 0, 0, 1], 46106));
        let (_, mut config) = crate::tests::mock::start_public_api(address);
        config.max_request_body_size = 128;
        config.max_response_body_size = 256;
        config.enable_ws = false;
        let handle = crate::serve(module, &address, &config).await.unwrap();
        let raw =
            json!({"jsonrpc":"2.0","id":1,"method":"small","params":["x".repeat(200)]}).to_string();
        assert_eq!(post(46106, raw).await.0, 413);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let (_, body) = post(46106, r#"{"jsonrpc":"2.0","id":2,"method":"big"}"#.into()).await;
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap()["error"]["code"],
            -32008
        );
        let (_, body) = post(46106, r#"{"jsonrpc":"2.0","method":"big"}"#.into()).await;
        assert!(body.is_empty());
        let (_, body) = post(46106, r#"{"jsonrpc":"2.0","id":3,"method":"small"}"#.into()).await;
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["result"], 7);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        handle.stop().await;
    }

    #[tokio::test]
    async fn notification_extensions() {
        use jsonrpsee::server::{
            middleware::rpc::RpcServiceT, BatchRequestConfig, HttpBody, HttpRequest, MethodResponse,
        };
        use jsonrpsee::types::{ErrorCode, Request};
        use std::future::{ready, Ready};
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct Capture(Arc<AtomicUsize>);
        impl<'a> RpcServiceT<'a> for Capture {
            type Future = Ready<MethodResponse>;
            fn call(&self, request: Request<'a>) -> Self::Future {
                assert_eq!(request.extensions().get::<u32>(), Some(&77));
                self.0.fetch_add(1, Ordering::SeqCst);
                ready(MethodResponse::error(
                    request.id(),
                    ErrorCode::MethodNotFound,
                ))
            }
        }
        let calls = Arc::new(AtomicUsize::new(0));
        for raw in [
            r#"{"jsonrpc":"2.0","method":"fixture"}"#,
            r#"[{"jsonrpc":"2.0","method":"fixture"}]"#,
        ] {
            let mut request = HttpRequest::new(HttpBody::from(raw.to_string()));
            *request.method_mut() = http::Method::POST;
            request.headers_mut().insert(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_static("application/json"),
            );
            request.extensions_mut().insert(77u32);
            let response = jsonrpsee::server::http::call_with_service(
                request,
                BatchRequestConfig::Limit(16),
                1024,
                Capture(calls.clone()),
                1024,
            )
            .await;
            assert_eq!(response.status(), 200);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn shared_websocket_behavior_websocket_neighbors() {
        use jsonrpsee::client_transport::ws::WsTransportClientBuilder;
        use jsonrpsee::core::client::{ReceivedMessage, TransportReceiverT, TransportSenderT};
        use jsonrpsee::core::server::SubscriptionMessage;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let subscriptions = Arc::new(AtomicUsize::new(0));
        let mut module = jsonrpsee::RpcModule::new(calls.clone());
        module
            .register_method("tick", |_, ctx, _| ctx.fetch_add(1, Ordering::SeqCst))
            .unwrap();
        module
            .register_method("count", |_, ctx, _| ctx.load(Ordering::SeqCst))
            .unwrap();
        let subs = subscriptions.clone();
        module
            .register_subscription("sub", "event", "unsub", move |_, pending, _, _| {
                let subs = subs.clone();
                async move {
                    subs.fetch_add(1, Ordering::SeqCst);
                    let sink = pending.accept().await?;
                    sink.send(SubscriptionMessage::from_json(&7)?).await?;
                    Ok::<(), jsonrpsee::core::StringError>(())
                }
            })
            .unwrap();
        let address = SocketAddr::from(([127, 0, 0, 1], 46107));
        let (_, mut config) = crate::tests::mock::start_public_api(address);
        config.enable_http = true;
        config.enable_ws = true;
        let handle = crate::serve(module, &address, &config).await.unwrap();
        let (mut sender, mut receiver) = WsTransportClientBuilder::default()
            .build(Url::parse("ws://127.0.0.1:46107").unwrap())
            .await
            .unwrap();
        sender
            .send(
                r#"[{"jsonrpc":"2.0","method":"tick"},{"jsonrpc":"2.0","id":1,"method":"count"}]"#
                    .into(),
            )
            .await
            .unwrap();
        let receive = |message| match message {
            ReceivedMessage::Text(raw) => serde_json::from_str::<Value>(&raw).unwrap(),
            _ => panic!("unexpected WS frame"),
        };
        let reply = receive(
            tokio::time::timeout(Duration::from_secs(2), receiver.receive())
                .await
                .unwrap()
                .unwrap(),
        );
        assert_eq!(reply, json!([{"jsonrpc":"2.0","id":1,"result":1}]));
        sender.send(r#"[{"jsonrpc":"2.0","id":true,"method":"tick"},{"jsonrpc":"2.0","id":2,"method":"count"}]"#.into()).await.unwrap();
        let reply = receive(
            tokio::time::timeout(Duration::from_secs(2), receiver.receive())
                .await
                .unwrap()
                .unwrap(),
        );
        assert_eq!(reply[0]["id"], Value::Null);
        assert_eq!(reply[0]["error"]["code"], -32600);
        assert_eq!(reply[1]["result"], 1);
        sender
            .send(
                r#"[{"jsonrpc":"2.0","method":"sub"},{"jsonrpc":"2.0","id":3,"method":"count"}]"#
                    .into(),
            )
            .await
            .unwrap();
        assert_eq!(
            receive(
                tokio::time::timeout(Duration::from_secs(2), receiver.receive())
                    .await
                    .unwrap()
                    .unwrap()
            ),
            json!([{"jsonrpc":"2.0","id":3,"result":1}])
        );
        assert_eq!(subscriptions.load(Ordering::SeqCst), 0);
        sender
            .send(r#"{"jsonrpc":"2.0","id":4,"method":"sub"}"#.into())
            .await
            .unwrap();
        let reply = receive(
            tokio::time::timeout(Duration::from_secs(2), receiver.receive())
                .await
                .unwrap()
                .unwrap(),
        );
        assert_eq!(reply["id"], 4);
        assert!(reply.get("result").is_some());
        let event = receive(
            tokio::time::timeout(Duration::from_secs(2), receiver.receive())
                .await
                .unwrap()
                .unwrap(),
        );
        assert_eq!(event["params"]["result"], 7);
        assert_eq!(subscriptions.load(Ordering::SeqCst), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        drop(sender);
        drop(receiver);
        handle.stop().await;
    }
}
