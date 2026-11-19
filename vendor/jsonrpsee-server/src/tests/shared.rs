use crate::tests::helpers::{init_logger, server_with_handles};
use hyper::StatusCode;
use jsonrpsee_test_utils::helpers::{http_request, ok_response, to_http_uri};
use jsonrpsee_test_utils::mocks::{Id, WebSocketTestClient, WebSocketTestError};
use jsonrpsee_test_utils::TimeoutFutureExt;
use std::time::Duration;

#[tokio::test]
async fn stop_works() {
	init_logger();
	let (_addr, server_handle) = server_with_handles().with_default_timeout().await.unwrap();

	let handle = server_handle.clone();
	handle.stop().unwrap();
	handle.stopped().await;

	// After that we should be able to wait for task handle to finish.
	// First `unwrap` is timeout, second is `JoinHandle`'s one.

	// After server was stopped, attempt to stop it again should result in an error.
	assert!(server_handle.stop().is_err());
}

#[tokio::test]
async fn run_forever() {
	const TIMEOUT: Duration = Duration::from_millis(200);

	init_logger();
	let (_addr, server_handle) = server_with_handles().with_default_timeout().await.unwrap();

	assert!(matches!(server_handle.stopped().with_timeout(TIMEOUT).await, Err(_timeout_err)));

	let (_addr, server_handle) = server_with_handles().with_default_timeout().await.unwrap();

	server_handle.stop().unwrap();

	// Send the shutdown request from one handle and await the server on the second one.
	server_handle.stopped().with_timeout(TIMEOUT).await.unwrap();
}

#[tokio::test]
async fn http_only_works() {
	use crate::{RpcModule, ServerBuilder};

	let server =
		ServerBuilder::default().http_only().build("127.0.0.1:0").with_default_timeout().await.unwrap().unwrap();
	let mut module = RpcModule::new(());
	module
		.register_method("say_hello", |_, _, _| {
			tracing::debug!("server respond to hello");
			"hello"
		})
		.unwrap();

	let addr = server.local_addr().unwrap();
	let _server_handle = server.start(module);

	let req = r#"{"jsonrpc":"2.0","method":"say_hello","id":1}"#;
	let response = http_request(req.into(), to_http_uri(addr)).with_default_timeout().await.unwrap().unwrap();
	assert_eq!(response.status, StatusCode::OK);
	assert_eq!(response.body, ok_response("hello".to_string().into(), Id::Num(1)));

	let err = WebSocketTestClient::new(addr).with_default_timeout().await.unwrap().unwrap_err();
	assert!(matches!(err, WebSocketTestError::RejectedWithStatusCode(code) if code == 403));
}

#[tokio::test]
async fn ws_only_works() {
	use crate::{RpcModule, ServerBuilder};

	let server = ServerBuilder::default().ws_only().build("127.0.0.1:0").with_default_timeout().await.unwrap().unwrap();
	let mut module = RpcModule::new(());
	module
		.register_method("say_hello", |_, _, _| {
			tracing::debug!("server respond to hello");
			"hello"
		})
		.unwrap();

	let addr = server.local_addr().unwrap();
	let _server_handle = server.start(module);

	let req = r#"{"jsonrpc":"2.0","method":"say_hello","id":1}"#;
	let response = http_request(req.into(), to_http_uri(addr)).with_default_timeout().await.unwrap().unwrap();
	assert_eq!(response.status, StatusCode::FORBIDDEN);

	let mut client = WebSocketTestClient::new(addr).with_default_timeout().await.unwrap().unwrap();
	let response = client.send_request_text(req.to_string()).await.unwrap();
	assert_eq!(response, ok_response("hello".to_string().into(), Id::Num(1)));
}
