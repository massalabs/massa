use std::{ops::Add, time::Duration};

use crate::tests::mock::{grpc_public_service, MockExecutionCtrl};
use massa_models::stats::ExecutionStats;
use massa_proto_rs::massa::api::v1::{
    public_service_client::PublicServiceClient, TransactionsThroughputRequest,
};
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
