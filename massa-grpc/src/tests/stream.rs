use std::{ops::Add, time::Duration};

use crate::tests::mock::{grpc_public_service, MockExecutionCtrl};
use massa_models::{address::Address, stats::ExecutionStats};
use massa_proto_rs::massa::{
    api::v1::{
        public_service_client::PublicServiceClient, NewOperationsRequest,
        TransactionsThroughputRequest,
    },
    model::v1::{operation_type, Transaction},
};
use massa_protocol_exports::test_exports::tools::create_operation_with_expire_period;
use massa_time::MassaTime;
use tokio_stream::StreamExt;

const GRPC_SERVER_URL: &str = "grpc://localhost:8888";

#[tokio::test]
async fn transactions_throughput_stream() {
    let mut public_server = grpc_public_service();
    let config = public_server.grpc_config.clone();

    let mut exec_ctrl = MockExecutionCtrl::new();

    exec_ctrl.expect_clone().returning(|| {
        let mut exec_ctrl = MockExecutionCtrl::new();
        exec_ctrl.expect_get_stats().returning(|| {
            let now = MassaTime::now().unwrap();
            let futur = MassaTime::from_millis(
                now.to_millis()
                    .add(Duration::from_secs(30).as_millis() as u64),
            );

            ExecutionStats {
                time_window_start: now.clone(),
                time_window_end: futur,
                final_block_count: 10,
                final_executed_operations_count: 2000,
                active_cursor: massa_models::slot::Slot {
                    period: 2,
                    thread: 10,
                },
                final_cursor: massa_models::slot::Slot {
                    period: 3,
                    thread: 15,
                },
            }
        });
        exec_ctrl
    });

    exec_ctrl.expect_clone_box().returning(|| {
        let mut exec_ctrl = MockExecutionCtrl::new();
        exec_ctrl.expect_get_stats().returning(|| {
            let now = MassaTime::now().unwrap();
            let futur = MassaTime::from_millis(
                now.to_millis()
                    .add(Duration::from_secs(30).as_millis() as u64),
            );

            ExecutionStats {
                time_window_start: now.clone(),
                time_window_end: futur,
                final_block_count: 10,
                final_executed_operations_count: 2000,
                active_cursor: massa_models::slot::Slot {
                    period: 2,
                    thread: 10,
                },
                final_cursor: massa_models::slot::Slot {
                    period: 3,
                    thread: 15,
                },
            }
        });
        Box::new(exec_ctrl)
    });

    public_server.execution_controller = Box::new(exec_ctrl);

    let stop_handle = public_server.serve(&config).await.unwrap();

    let mut public_client = PublicServiceClient::connect(GRPC_SERVER_URL).await.unwrap();

    // channel for bi-directional streaming
    let (tx, rx) = tokio::sync::mpsc::channel(10);

    // Create a stream from the receiver.
    let request_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    let mut resp_stream = public_client
        .transactions_throughput(request_stream)
        .await
        .unwrap()
        .into_inner();

    tx.send(TransactionsThroughputRequest { interval: Some(1) })
        .await
        .unwrap();

    let mut count = 0;
    let mut now = std::time::Instant::now();
    while let Some(received) = resp_stream.next().await {
        let received = received.unwrap();
        assert_eq!(received.throughput, 66);

        let time_to_get_msg = now.elapsed().as_secs_f64().round();

        if count < 2 {
            assert!(time_to_get_msg < 1.5);
        } else if count >= 2 && count < 4 {
            assert!(time_to_get_msg < 3.5 && time_to_get_msg > 2.5);
        } else {
            break;
        }

        now = std::time::Instant::now();

        count += 1;
        if count == 2 {
            // update interval to 3 seconds
            tx.send(TransactionsThroughputRequest { interval: Some(3) })
                .await
                .unwrap();
        }
    }

    stop_handle.stop();
}

#[tokio::test]
async fn new_operations() {
    let mut public_server = grpc_public_service();
    let config = public_server.grpc_config.clone();
    let (op_tx, _op_rx) = tokio::sync::broadcast::channel(10);
    let keypair = massa_signature::KeyPair::generate(0).unwrap();
    let address = Address::from_public_key(&keypair.get_public_key());
    public_server.pool_channels.operation_sender = op_tx.clone();

    let stop_handle = public_server.serve(&config).await.unwrap();

    let mut public_client = PublicServiceClient::connect(GRPC_SERVER_URL).await.unwrap();
    let op = create_operation_with_expire_period(&keypair, 10);
    let (op_send_signal, mut rx_op_send) = tokio::sync::mpsc::channel(10);

    let (tx_request, rx) = tokio::sync::mpsc::channel(10);
    let request_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    let tx_cloned = op_tx.clone();
    let op_cloned = op.clone();
    tokio::spawn(async move {
        loop {
            // when receive signal, broadcast op
            let _: () = rx_op_send.recv().await.unwrap();

            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            // send op
            tx_cloned.send(op_cloned.clone()).unwrap();
        }
    });

    let mut resp_stream = public_client
        .new_operations(request_stream)
        .await
        .unwrap()
        .into_inner();

    let filter = massa_proto_rs::massa::api::v1::NewOperationsFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_operations_filter::Filter::OperationIds(
                massa_proto_rs::massa::model::v1::OperationIds {
                    operation_ids: vec![
                        "O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC".to_string()
                    ],
                },
            ),
        ),
    };

    // send filter with unknow op id
    tx_request
        .send(NewOperationsRequest {
            filters: vec![filter],
        })
        .await
        .unwrap();

    op_send_signal.send(()).await.unwrap();

    // wait for response
    // should be timed out because of unknow op id
    let result = tokio::time::timeout(Duration::from_secs(2), resp_stream.next()).await;
    assert!(result.is_err());

    // send filter with known op id
    let filter_id = massa_proto_rs::massa::api::v1::NewOperationsFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_operations_filter::Filter::OperationIds(
                massa_proto_rs::massa::model::v1::OperationIds {
                    operation_ids: vec![op.id.to_string()],
                },
            ),
        ),
    };

    tx_request
        .send(NewOperationsRequest {
            filters: vec![filter_id.clone()],
        })
        .await
        .unwrap();

    op_send_signal.send(()).await.unwrap();

    // wait for response
    let result = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap();
    let received = result.unwrap();
    assert_eq!(
        received.signed_operation.unwrap().content_creator_pub_key,
        keypair.get_public_key().to_string()
    );

    let mut filter_type = massa_proto_rs::massa::api::v1::NewOperationsFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_operations_filter::Filter::OperationTypes(
                massa_proto_rs::massa::model::v1::OpTypes {
                    op_types: vec![massa_proto_rs::massa::model::v1::OpType::CallSc as i32],
                },
            ),
        ),
    };

    tx_request
        .send(NewOperationsRequest {
            filters: vec![filter_type],
        })
        .await
        .unwrap();

    op_send_signal.send(()).await.unwrap();

    let result = tokio::time::timeout(Duration::from_secs(2), resp_stream.next()).await;

    assert!(result.is_err());

    filter_type = massa_proto_rs::massa::api::v1::NewOperationsFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_operations_filter::Filter::OperationTypes(
                massa_proto_rs::massa::model::v1::OpTypes {
                    op_types: vec![massa_proto_rs::massa::model::v1::OpType::Transaction as i32],
                },
            ),
        ),
    };

    tx_request
        .send(NewOperationsRequest {
            filters: vec![filter_type.clone()],
        })
        .await
        .unwrap();

    op_send_signal.send(()).await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap();
    let received = result.unwrap();
    assert_eq!(
        received.signed_operation.unwrap().content_creator_pub_key,
        keypair.get_public_key().to_string()
    );

    tx_request
        .send(NewOperationsRequest {
            filters: vec![filter_type, filter_id],
        })
        .await
        .unwrap();
    op_send_signal.send(()).await.unwrap();

    let received = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        received.signed_operation.unwrap().content_creator_pub_key,
        keypair.get_public_key().to_string()
    );

    let mut filter_addr = massa_proto_rs::massa::api::v1::NewOperationsFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_operations_filter::Filter::Addresses(
                massa_proto_rs::massa::model::v1::Addresses {
                    addresses: vec![
                        "AU12BTfZ7k1z6PsLEUZeHYNirz6WJ3NdrWto9H4TkVpkV9xE2TJg2".to_string()
                    ],
                },
            ),
        ),
    };

    tx_request
        .send(NewOperationsRequest {
            filters: vec![filter_addr.clone()],
        })
        .await
        .unwrap();
    op_send_signal.send(()).await.unwrap();

    let result = tokio::time::timeout(Duration::from_secs(2), resp_stream.next()).await;
    assert!(result.is_err());

    filter_addr = massa_proto_rs::massa::api::v1::NewOperationsFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_operations_filter::Filter::Addresses(
                massa_proto_rs::massa::model::v1::Addresses {
                    addresses: vec![address.to_string()],
                },
            ),
        ),
    };

    tx_request
        .send(NewOperationsRequest {
            filters: vec![filter_addr.clone()],
        })
        .await
        .unwrap();
    op_send_signal.send(()).await.unwrap();
    let received = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        received.signed_operation.unwrap().content_creator_pub_key,
        keypair.get_public_key().to_string()
    );

    stop_handle.stop();
}
