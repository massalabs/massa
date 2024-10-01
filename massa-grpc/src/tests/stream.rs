// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::tests::mock::grpc_public_service;
use core::panic;
use massa_consensus_exports::MockConsensusController;
use massa_execution_exports::{ExecutionOutput, MockExecutionController, SlotExecutionOutput};
use massa_models::{
    address::Address, amount::Amount, block::FilledBlock, secure_share::SecureShareSerializer,
    slot::Slot, stats::ExecutionStats,
};
use massa_pool_exports::MockPoolController;
use massa_proto_rs::massa::{
    api::v1::{
        public_service_client::PublicServiceClient, NewBlocksRequest, NewFilledBlocksRequest,
        NewOperationsRequest, NewSlotExecutionOutputsRequest, SendEndorsementsRequest,
        SendOperationsRequest, TransactionsThroughputRequest,
    },
    model::v1::{Addresses, Slot as ProtoSlot, SlotRange},
};
use massa_protocol_exports::{
    test_exports::tools::{
        create_block, create_block_with_operations, create_endorsement,
        create_operation_with_expire_period,
    },
    MockProtocolController,
};
use massa_serialization::Serializer;
use massa_signature::KeyPair;
use massa_time::MassaTime;
use std::{net::SocketAddr, ops::Add, str::FromStr, time::Duration};
use tokio_stream::StreamExt;

#[tokio::test]
async fn transactions_throughput_stream() {
    let addr: SocketAddr = "[::]:4017".parse().unwrap();
    let mut public_server = grpc_public_service(&addr);
    let config = public_server.grpc_config.clone();

    let mut exec_ctrl = Box::new(MockExecutionController::new());

    exec_ctrl.expect_clone_box().returning(|| {
        let mut exec_ctrl = Box::new(MockExecutionController::new());
        exec_ctrl.expect_get_stats().returning(|| {
            let now = MassaTime::now();
            let future_ = MassaTime::from_millis(
                now.as_millis()
                    .add(Duration::from_secs(30).as_millis() as u64),
            );

            ExecutionStats {
                time_window_start: now,
                time_window_end: future_,
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
        let mut exec_ctrl = Box::new(MockExecutionController::new());
        exec_ctrl.expect_get_stats().returning(|| {
            let now = MassaTime::now();
            let future_ = MassaTime::from_millis(
                now.as_millis()
                    .add(Duration::from_secs(30).as_millis() as u64),
            );

            ExecutionStats {
                time_window_start: now,
                time_window_end: future_,
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

    public_server.execution_controller = exec_ctrl;

    let stop_handle = public_server.serve(&config).await.unwrap();

    let mut public_client = PublicServiceClient::connect(format!(
        "grpc://localhost:{}",
        addr.to_string().split(':').last().unwrap()
    ))
    .await
    .unwrap();

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
            // firsts messages should be received in less than 1.5 seconds
            assert!(time_to_get_msg < 1.5);
        } else if (2..4).contains(&count) {
            // next 2 messages should be received in less than 3.5 seconds and more than 2.5 seconds (filter interval is 3 seconds)
            assert!(time_to_get_msg < 3.5 && time_to_get_msg > 2.5);
        } else {
            // end of test
            break;
        }

        now = std::time::Instant::now();

        // increment count of received messages
        count += 1;
        if count == 2 {
            // when 2 messages received, update interval to 3 seconds
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
    let addr: SocketAddr = "[::]:4018".parse().unwrap();
    let mut public_server = grpc_public_service(&addr);
    let config = public_server.grpc_config.clone();
    let (op_tx, _op_rx) = tokio::sync::broadcast::channel(10);
    let keypair = massa_signature::KeyPair::generate(0).unwrap();
    let address = Address::from_public_key(&keypair.get_public_key());
    public_server.pool_broadcasts.operation_sender = op_tx.clone();

    let stop_handle = public_server.serve(&config).await.unwrap();
    let mut public_client = PublicServiceClient::connect(format!(
        "grpc://localhost:{}",
        addr.to_string().split(':').last().unwrap()
    ))
    .await
    .unwrap();
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
    // check if received op is the same as the one sent
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
    // assert ok
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
    // unknown address
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

#[tokio::test]
async fn new_blocks() {
    let addr: SocketAddr = "[::]:4019".parse().unwrap();
    let mut public_server = grpc_public_service(&addr);
    let config = public_server.grpc_config.clone();
    let (block_tx, _block_rx) = tokio::sync::broadcast::channel(10);

    public_server.consensus_broadcasts.block_sender = block_tx.clone();

    let stop_handle = public_server.serve(&config).await.unwrap();

    let keypair = KeyPair::generate(0).unwrap();
    let address = Address::from_public_key(&keypair.get_public_key());
    let op = create_operation_with_expire_period(&keypair, 4);

    let block_op = create_block_with_operations(
        &keypair,
        Slot {
            period: 1,
            thread: 4,
        },
        vec![op.clone()],
    );

    let mut public_client = PublicServiceClient::connect(format!(
        "grpc://localhost:{}",
        addr.to_string().split(':').last().unwrap()
    ))
    .await
    .unwrap();

    let (tx_request, rx) = tokio::sync::mpsc::channel(10);
    let request_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    let mut resp_stream = public_client
        .new_blocks(request_stream)
        .await
        .unwrap()
        .into_inner();

    let mut filter_slot = massa_proto_rs::massa::api::v1::NewBlocksFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_blocks_filter::Filter::SlotRange(SlotRange {
                start_slot: Some(ProtoSlot {
                    period: 1,
                    thread: 1,
                }),
                end_slot: None,
            }),
        ),
    };
    tx_request
        .send(NewBlocksRequest {
            filters: vec![filter_slot.clone()],
        })
        .await
        .unwrap();

    // send block
    block_tx.send(block_op.clone()).unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    // assert block is received
    assert!(result.signed_block.is_some());

    filter_slot = massa_proto_rs::massa::api::v1::NewBlocksFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_blocks_filter::Filter::SlotRange(SlotRange {
                start_slot: Some(ProtoSlot {
                    period: 1,
                    thread: 15,
                }),
                end_slot: None,
            }),
        ),
    };

    // update filter
    tx_request
        .send(NewBlocksRequest {
            filters: vec![filter_slot],
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    // send block
    block_tx.send(block_op.clone()).unwrap();

    let result = tokio::time::timeout(Duration::from_secs(2), resp_stream.next()).await;
    // elapsed, start slot is after block slot
    assert!(result.is_err());

    filter_slot = massa_proto_rs::massa::api::v1::NewBlocksFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_blocks_filter::Filter::SlotRange(SlotRange {
                start_slot: None,
                end_slot: Some(ProtoSlot {
                    period: 1,
                    thread: 15,
                }),
            }),
        ),
    };

    tx_request
        .send(NewBlocksRequest {
            filters: vec![filter_slot],
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    // send block
    block_tx.send(block_op.clone()).unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    // assert block is received
    assert!(result.signed_block.is_some());

    let mut filter_addr = massa_proto_rs::massa::api::v1::NewBlocksFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_blocks_filter::Filter::Addresses(Addresses {
                addresses: vec!["AU12BTfZ7k1z6PsLEUZeHYNirz6WJ3NdrWto9H4TkVpkV9xE2TJg2".to_string()],
            }),
        ),
    };

    tx_request
        .send(NewBlocksRequest {
            filters: vec![filter_addr],
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    // send block
    block_tx.send(block_op.clone()).unwrap();

    let result = tokio::time::timeout(Duration::from_secs(2), resp_stream.next()).await;
    // elapsed
    assert!(result.is_err());

    filter_addr = massa_proto_rs::massa::api::v1::NewBlocksFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_blocks_filter::Filter::Addresses(Addresses {
                addresses: vec![address.to_string()],
            }),
        ),
    };

    tx_request
        .send(NewBlocksRequest {
            filters: vec![filter_addr.clone()],
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    // send block
    block_tx.send(block_op.clone()).unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    assert!(result.signed_block.is_some());

    let mut filter_ids = massa_proto_rs::massa::api::v1::NewBlocksFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_blocks_filter::Filter::BlockIds(
                massa_proto_rs::massa::model::v1::BlockIds {
                    block_ids: vec![
                        "B1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC".to_string()
                    ],
                },
            ),
        ),
    };

    tx_request
        .send(NewBlocksRequest {
            filters: vec![filter_ids],
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    // send block
    block_tx.send(block_op.clone()).unwrap();

    let result = tokio::time::timeout(Duration::from_secs(2), resp_stream.next()).await;
    // elapsed
    assert!(result.is_err());

    filter_ids = massa_proto_rs::massa::api::v1::NewBlocksFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_blocks_filter::Filter::BlockIds(
                massa_proto_rs::massa::model::v1::BlockIds {
                    block_ids: vec![block_op.id.to_string()],
                },
            ),
        ),
    };

    tx_request
        .send(NewBlocksRequest {
            filters: vec![filter_ids],
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    // send block
    block_tx.send(block_op.clone()).unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    assert!(result.signed_block.is_some());

    filter_addr = massa_proto_rs::massa::api::v1::NewBlocksFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_blocks_filter::Filter::Addresses(Addresses {
                addresses: vec!["massa".to_string()],
            }),
        ),
    };

    tx_request
        .send(NewBlocksRequest {
            filters: vec![filter_addr],
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    let result = tokio::time::timeout(Duration::from_secs(3), resp_stream.next())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(result.unwrap_err().message(), "invalid address: massa");

    stop_handle.stop();
}

#[tokio::test]
async fn new_endorsements() {
    let addr: SocketAddr = "[::]:4020".parse().unwrap();
    let mut public_server = grpc_public_service(&addr);
    let config = public_server.grpc_config.clone();

    let (endorsement_tx, _endorsement_rx) = tokio::sync::broadcast::channel(10);

    public_server.pool_broadcasts.endorsement_sender = endorsement_tx.clone();

    let stop_handle = public_server.serve(&config).await.unwrap();

    let mut public_client = PublicServiceClient::connect(format!(
        "grpc://localhost:{}",
        addr.to_string().split(':').last().unwrap()
    ))
    .await
    .unwrap();

    let endorsement = create_endorsement();

    let (tx_request, rx) = tokio::sync::mpsc::channel(10);
    let request_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    let mut resp_stream = public_client
        .new_endorsements(request_stream)
        .await
        .unwrap()
        .into_inner();

    let mut filter_ids = massa_proto_rs::massa::api::v1::NewEndorsementsFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_endorsements_filter::Filter::EndorsementIds(
                massa_proto_rs::massa::model::v1::EndorsementIds {
                    endorsement_ids: vec![
                        "E1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC".to_string()
                    ],
                },
            ),
        ),
    };

    tx_request
        .send(massa_proto_rs::massa::api::v1::NewEndorsementsRequest {
            filters: vec![filter_ids],
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    endorsement_tx.send(endorsement.clone()).unwrap();

    let result = tokio::time::timeout(Duration::from_secs(2), resp_stream.next()).await;
    assert!(result.is_err());

    filter_ids = massa_proto_rs::massa::api::v1::NewEndorsementsFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_endorsements_filter::Filter::EndorsementIds(
                massa_proto_rs::massa::model::v1::EndorsementIds {
                    endorsement_ids: vec![endorsement.id.to_string()],
                },
            ),
        ),
    };

    tx_request
        .send(massa_proto_rs::massa::api::v1::NewEndorsementsRequest {
            filters: vec![filter_ids],
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    endorsement_tx.send(endorsement.clone()).unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    assert!(result.signed_endorsement.is_some());

    let mut filter_addr = massa_proto_rs::massa::api::v1::NewEndorsementsFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_endorsements_filter::Filter::Addresses(
                massa_proto_rs::massa::model::v1::Addresses {
                    addresses: vec![
                        "AU12BTfZ7k1z6PsLEUZeHYNirz6WJ3NdrWto9H4TkVpkV9xE2TJg2".to_string()
                    ],
                },
            ),
        ),
    };

    tx_request
        .send(massa_proto_rs::massa::api::v1::NewEndorsementsRequest {
            filters: vec![filter_addr],
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    endorsement_tx.send(endorsement.clone()).unwrap();

    let result = tokio::time::timeout(Duration::from_secs(2), resp_stream.next()).await;
    // unknown address
    assert!(result.is_err());

    filter_addr = massa_proto_rs::massa::api::v1::NewEndorsementsFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_endorsements_filter::Filter::Addresses(
                massa_proto_rs::massa::model::v1::Addresses {
                    addresses: vec![endorsement.content_creator_address.to_string()],
                },
            ),
        ),
    };

    tx_request
        .send(massa_proto_rs::massa::api::v1::NewEndorsementsRequest {
            filters: vec![filter_addr],
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    endorsement_tx.send(endorsement.clone()).unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    assert!(result.signed_endorsement.is_some());

    let mut filter_block_ids = massa_proto_rs::massa::api::v1::NewEndorsementsFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_endorsements_filter::Filter::BlockIds(
                massa_proto_rs::massa::model::v1::BlockIds {
                    block_ids: vec![
                        "B1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC".to_string()
                    ],
                },
            ),
        ),
    };

    tx_request
        .send(massa_proto_rs::massa::api::v1::NewEndorsementsRequest {
            filters: vec![filter_block_ids],
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    endorsement_tx.send(endorsement.clone()).unwrap();

    let result = tokio::time::timeout(Duration::from_secs(2), resp_stream.next()).await;
    assert!(result.is_err());

    filter_block_ids = massa_proto_rs::massa::api::v1::NewEndorsementsFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_endorsements_filter::Filter::BlockIds(
                massa_proto_rs::massa::model::v1::BlockIds {
                    block_ids: vec![endorsement.content.endorsed_block.to_string()],
                },
            ),
        ),
    };

    tx_request
        .send(massa_proto_rs::massa::api::v1::NewEndorsementsRequest {
            filters: vec![filter_block_ids],
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    endorsement_tx.send(endorsement.clone()).unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    assert!(result.signed_endorsement.is_some());

    stop_handle.stop();
}

#[tokio::test]
async fn new_filled_blocks() {
    let addr: SocketAddr = "[::]:4021".parse().unwrap();
    let mut public_server = grpc_public_service(&addr);
    let config = public_server.grpc_config.clone();

    let (filled_block_tx, _filled_block_rx) = tokio::sync::broadcast::channel(10);

    public_server.consensus_broadcasts.filled_block_sender = filled_block_tx.clone();

    let stop_handle = public_server.serve(&config).await.unwrap();

    let (tx_request, rx) = tokio::sync::mpsc::channel(10);
    let request_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    let keypair = KeyPair::generate(0).unwrap();
    let address = Address::from_public_key(&keypair.get_public_key());
    let block = create_block(&keypair);

    let filled_block = FilledBlock {
        header: block.content.header.clone(),
        operations: vec![],
    };

    let mut public_client = PublicServiceClient::connect(format!(
        "grpc://localhost:{}",
        addr.to_string().split(':').last().unwrap()
    ))
    .await
    .unwrap();

    let mut resp_stream = public_client
        .new_filled_blocks(request_stream)
        .await
        .unwrap()
        .into_inner();

    let mut filter = massa_proto_rs::massa::api::v1::NewBlocksFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_blocks_filter::Filter::SlotRange(SlotRange {
                start_slot: Some(ProtoSlot {
                    period: 1,
                    thread: 0,
                }),
                end_slot: None,
            }),
        ),
    };

    tx_request
        .send(NewFilledBlocksRequest {
            filters: vec![filter],
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    filled_block_tx.send(filled_block.clone()).unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    assert!(result.filled_block.is_some());

    filter = massa_proto_rs::massa::api::v1::NewBlocksFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_blocks_filter::Filter::SlotRange(SlotRange {
                start_slot: Some(ProtoSlot {
                    period: 1,
                    thread: 5,
                }),
                end_slot: None,
            }),
        ),
    };

    tx_request
        .send(NewFilledBlocksRequest {
            filters: vec![filter],
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    filled_block_tx.send(filled_block.clone()).unwrap();

    let result = tokio::time::timeout(Duration::from_secs(2), resp_stream.next()).await;
    // stat slot is after block slot
    assert!(result.is_err());

    filter = massa_proto_rs::massa::api::v1::NewBlocksFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_blocks_filter::Filter::BlockIds(
                massa_proto_rs::massa::model::v1::BlockIds {
                    block_ids: vec![
                        "B1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC".to_string()
                    ],
                },
            ),
        ),
    };

    tx_request
        .send(NewFilledBlocksRequest {
            filters: vec![filter],
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    filled_block_tx.send(filled_block.clone()).unwrap();

    let result = tokio::time::timeout(Duration::from_secs(2), resp_stream.next()).await;
    // unknown block id
    assert!(result.is_err());

    filter = massa_proto_rs::massa::api::v1::NewBlocksFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_blocks_filter::Filter::BlockIds(
                massa_proto_rs::massa::model::v1::BlockIds {
                    block_ids: vec![filled_block.header.id.to_string()],
                },
            ),
        ),
    };

    tx_request
        .send(NewFilledBlocksRequest {
            filters: vec![filter],
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    filled_block_tx.send(filled_block.clone()).unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    assert!(result.filled_block.is_some());

    filter = massa_proto_rs::massa::api::v1::NewBlocksFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_blocks_filter::Filter::Addresses(Addresses {
                addresses: vec!["AU12BTfZ7k1z6PsLEUZeHYNirz6WJ3NdrWto9H4TkVpkV9xE2TJg2".to_string()],
            }),
        ),
    };

    tx_request
        .send(NewFilledBlocksRequest {
            filters: vec![filter],
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    filled_block_tx.send(filled_block.clone()).unwrap();

    let result = tokio::time::timeout(Duration::from_secs(2), resp_stream.next()).await;
    // unknown address
    assert!(result.is_err());

    filter = massa_proto_rs::massa::api::v1::NewBlocksFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_blocks_filter::Filter::Addresses(Addresses {
                addresses: vec![address.to_string()],
            }),
        ),
    };

    tx_request
        .send(NewFilledBlocksRequest {
            filters: vec![filter],
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    filled_block_tx.send(filled_block.clone()).unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    assert!(result.filled_block.is_some());

    stop_handle.stop();
}

#[tokio::test]
async fn new_slot_execution_outputs() {
    let addr: SocketAddr = "[::]:4022".parse().unwrap();
    let mut public_server = grpc_public_service(&addr);
    let config = public_server.grpc_config.clone();

    let (slot_tx, _slot_rx) = tokio::sync::broadcast::channel(10);

    public_server
        .execution_channels
        .slot_execution_output_sender = slot_tx.clone();

    let stop_handle = public_server.serve(&config).await.unwrap();

    let exec_output_1 = ExecutionOutput {
        slot: Slot::new(1, 5),
        block_info: None,
        state_changes: massa_final_state::StateChanges::default(),
        events: Default::default(),
        #[cfg(feature = "execution-trace")]
        slot_trace: None,
        #[cfg(feature = "dump-block")]
        storage: None,
        deferred_credits_execution: vec![],
        cancel_async_message_execution: vec![],
        auto_sell_execution: vec![],
    };

    let (tx_request, rx) = tokio::sync::mpsc::channel(10);
    let request_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    let keypair = KeyPair::generate(0).unwrap();
    let _address = Address::from_public_key(&keypair.get_public_key());

    let mut public_client = PublicServiceClient::connect(format!(
        "grpc://localhost:{}",
        addr.to_string().split(':').last().unwrap()
    ))
    .await
    .unwrap();

    let mut resp_stream = public_client
        .new_slot_execution_outputs(request_stream)
        .await
        .unwrap()
        .into_inner();

    let mut filter = massa_proto_rs::massa::api::v1::NewSlotExecutionOutputsFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_slot_execution_outputs_filter::Filter::SlotRange(
                SlotRange {
                    start_slot: Some(ProtoSlot {
                        period: 1,
                        thread: 0,
                    }),
                    end_slot: None,
                },
            ),
        ),
    };

    tx_request
        .send(NewSlotExecutionOutputsRequest {
            filters: vec![filter],
        })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    slot_tx
        .send(SlotExecutionOutput::ExecutedSlot(exec_output_1.clone()))
        .unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    assert!(result.output.is_some());

    filter = massa_proto_rs::massa::api::v1::NewSlotExecutionOutputsFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_slot_execution_outputs_filter::Filter::SlotRange(
                SlotRange {
                    start_slot: Some(ProtoSlot {
                        period: 1,
                        thread: 0,
                    }),
                    end_slot: Some(ProtoSlot {
                        period: 1,
                        thread: 7,
                    }),
                },
            ),
        ),
    };

    tx_request
        .send(NewSlotExecutionOutputsRequest {
            filters: vec![filter],
        })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    slot_tx
        .send(SlotExecutionOutput::ExecutedSlot(exec_output_1.clone()))
        .unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    // start slot and end slot are in range
    assert!(result.output.is_some());

    filter = massa_proto_rs::massa::api::v1::NewSlotExecutionOutputsFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_slot_execution_outputs_filter::Filter::SlotRange(
                SlotRange {
                    start_slot: Some(ProtoSlot {
                        period: 1,
                        thread: 7,
                    }),
                    end_slot: None,
                },
            ),
        ),
    };

    tx_request
        .send(NewSlotExecutionOutputsRequest {
            filters: vec![filter],
        })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    slot_tx
        .send(SlotExecutionOutput::ExecutedSlot(exec_output_1.clone()))
        .unwrap();

    let result = tokio::time::timeout(Duration::from_secs(2), resp_stream.next()).await;
    // start slot is after block slot
    assert!(result.is_err());

    // TODO add test when filter is updated

    /*     filter = massa_proto_rs::massa::api::v1::NewSlotExecutionOutputsFilter {
        filter: Some(
            massa_proto_rs::massa::api::v1::new_slot_execution_outputs_filter::Filter::EventFilter(
                massa_proto_rs::massa::api::v1::ExecutionEventFilter {
                    filter: Some(
                        massa_proto_rs::massa::api::v1::execution_event_filter::Filter::OriginalOperationId( "O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC"
                                    .to_string()
                        ),
                    ),
                },
            ),
        ),
    };

    tx_request
        .send(NewSlotExecutionOutputsRequest {
            filters: vec![filter],
        })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    slot_tx
        .send(SlotExecutionOutput::ExecutedSlot(exec_output_1.clone()))
        .unwrap();

    let result = tokio::time::timeout(Duration::from_secs(2), resp_stream.next()).await;
    dbg!(&result);
    assert!(result.is_err()); */

    stop_handle.stop();
}

#[tokio::test]
async fn send_operations_low_fee() {
    let addr: SocketAddr = "[::]:4000".parse().unwrap();
    let mut public_server = grpc_public_service(&addr);
    public_server.grpc_config.minimal_fees = Amount::from_str("0.01").unwrap();

    let mut pool_ctrl = Box::new(MockPoolController::new());
    pool_ctrl.expect_clone_box().returning(|| {
        let mut pool_ctrl = Box::new(MockPoolController::new());

        pool_ctrl.expect_add_operations().returning(|_| ());

        pool_ctrl
    });

    let mut protocol_ctrl = Box::new(MockProtocolController::new());
    protocol_ctrl.expect_clone_box().returning(|| {
        let mut ctrl = Box::new(MockProtocolController::new());

        ctrl.expect_propagate_operations().returning(|_| Ok(()));

        ctrl
    });

    public_server.pool_controller = pool_ctrl;
    public_server.protocol_controller = protocol_ctrl;

    let config = public_server.grpc_config.clone();
    let stop_handle = public_server.serve(&config).await.unwrap();

    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let request_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    let mut public_client = PublicServiceClient::connect(format!(
        "grpc://localhost:{}",
        addr.to_string().split(':').last().unwrap()
    ))
    .await
    .unwrap();

    let mut resp_stream = public_client
        .send_operations(request_stream)
        .await
        .unwrap()
        .into_inner();

    let keypair = KeyPair::generate(0).unwrap();
    // Note: expire_period is set to be higher than current slot (which is computed from config genesis timestamp)
    //       CHeck send_operation.rs where last_slot value is computed
    let op2 = create_operation_with_expire_period(&keypair, 11950000);
    let mut buffer: Vec<u8> = Vec::new();
    SecureShareSerializer::new()
        .serialize(&op2, &mut buffer)
        .unwrap();

    tx.send(SendOperationsRequest {
        operations: vec![buffer.clone()],
    })
    .await
    .unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .result
        .unwrap();

    match result {
        massa_proto_rs::massa::api::v1::send_operations_response::Result::OperationIds(_ope_id) => {
            panic!("should be error");
        }
        massa_proto_rs::massa::api::v1::send_operations_response::Result::Error(e) => {
            assert_eq!(e.message, "invalid operation(s): Invalid argument error: Operation fee is lower than the minimal fee. Your operation will never be included in a block.");
        }
    }

    stop_handle.stop();
}

#[tokio::test]
async fn send_operations() {
    let addr: SocketAddr = "[::]:4033".parse().unwrap();
    let mut public_server = grpc_public_service(&addr);

    let mut pool_ctrl = Box::new(MockPoolController::new());
    pool_ctrl.expect_clone_box().returning(|| {
        let mut pool_ctrl = Box::new(MockPoolController::new());

        pool_ctrl.expect_add_operations().returning(|_| ());

        pool_ctrl
    });

    let mut protocol_ctrl = Box::new(MockProtocolController::new());
    protocol_ctrl.expect_clone_box().returning(|| {
        let mut ctrl = Box::new(MockProtocolController::new());

        ctrl.expect_propagate_operations().returning(|_| Ok(()));

        ctrl
    });

    public_server.pool_controller = pool_ctrl;
    public_server.protocol_controller = protocol_ctrl;

    let config = public_server.grpc_config.clone();

    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let request_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    let stop_handle = public_server.serve(&config).await.unwrap();

    let mut public_client = PublicServiceClient::connect(format!(
        "grpc://localhost:{}",
        addr.to_string().split(':').last().unwrap()
    ))
    .await
    .unwrap();

    let mut resp_stream = public_client
        .send_operations(request_stream)
        .await
        .unwrap()
        .into_inner();

    let keypair = KeyPair::generate(0).unwrap();
    let op = create_operation_with_expire_period(&keypair, 4);

    tx.send(SendOperationsRequest {
        operations: vec![op.clone().serialized_data],
    })
    .await
    .unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    match result.result.unwrap() {
        massa_proto_rs::massa::api::v1::send_operations_response::Result::OperationIds(_) => {
            panic!("should be error");
        }
        massa_proto_rs::massa::api::v1::send_operations_response::Result::Error(err) => {
            assert!(err.message.contains("invalid operation"));
        }
    }

    let mut buffer: Vec<u8> = Vec::new();
    SecureShareSerializer::new()
        .serialize(&op, &mut buffer)
        .unwrap();

    tx.send(SendOperationsRequest {
        operations: vec![buffer],
    })
    .await
    .unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    match result.result.unwrap() {
        massa_proto_rs::massa::api::v1::send_operations_response::Result::Error(err) => {
            assert!(err
                .message
                .contains("Operation expire_period is lower than the current period of this node"));
        }
        _ => {
            panic!("should be error");
        }
    }

    // Note: expire_period is set to be higher than current slot (which is computed from config genesis timestamp)
    //       CHeck send_operation.rs where last_slot value is computed
    let op2 = create_operation_with_expire_period(&keypair, 11950000);
    let mut buffer: Vec<u8> = Vec::new();
    SecureShareSerializer::new()
        .serialize(&op2, &mut buffer)
        .unwrap();

    tx.send(SendOperationsRequest {
        operations: vec![buffer.clone()],
    })
    .await
    .unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .result
        .unwrap();

    match result {
        massa_proto_rs::massa::api::v1::send_operations_response::Result::OperationIds(ope_id) => {
            assert_eq!(ope_id.operation_ids.len(), 1);
            assert_eq!(ope_id.operation_ids[0], op2.id.to_string());
        }
        massa_proto_rs::massa::api::v1::send_operations_response::Result::Error(e) => {
            panic!("Send operations error: {:?}", e);
        }
    }

    tx.send(SendOperationsRequest {
        operations: vec![buffer.clone(), buffer.clone(), buffer.clone()],
    })
    .await
    .unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    match result.result.unwrap() {
        massa_proto_rs::massa::api::v1::send_operations_response::Result::Error(err) => {
            assert_eq!(err.message, "too many operations per message");
        }
        _ => {
            panic!("should be error");
        }
    }

    stop_handle.stop();
}

#[tokio::test]
async fn send_endorsements() {
    let addr: SocketAddr = "[::]:4024".parse().unwrap();
    let mut public_server = grpc_public_service(&addr);
    let config = public_server.grpc_config.clone();

    let mut protocol_ctrl = Box::new(MockProtocolController::new());
    protocol_ctrl.expect_clone_box().returning(|| {
        let mut ctrl = Box::new(MockProtocolController::new());

        ctrl.expect_propagate_endorsements().returning(|_| Ok(()));

        ctrl
    });

    let mut pool_ctrl = Box::new(MockPoolController::new());
    pool_ctrl.expect_clone_box().returning(|| {
        let mut ctrl = Box::new(MockPoolController::new());

        ctrl.expect_add_endorsements().returning(|_| ());

        ctrl
    });

    public_server.pool_controller = pool_ctrl;
    public_server.protocol_controller = protocol_ctrl;

    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let request_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    let stop_handle = public_server.serve(&config).await.unwrap();

    let mut public_client = PublicServiceClient::connect(format!(
        "grpc://localhost:{}",
        addr.to_string().split(':').last().unwrap()
    ))
    .await
    .unwrap();

    let mut resp_stream = public_client
        .send_endorsements(request_stream)
        .await
        .unwrap()
        .into_inner();

    let endorsement = create_endorsement();
    // serialize endorsement
    let mut buffer: Vec<u8> = Vec::new();
    SecureShareSerializer::new()
        .serialize(&endorsement, &mut buffer)
        .unwrap();

    tx.send(SendEndorsementsRequest {
        endorsements: vec![buffer],
    })
    .await
    .unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    assert!(result.result.is_some());

    // cause fail deserialize endorsement
    tx.send(SendEndorsementsRequest {
        endorsements: vec![endorsement.serialized_data],
    })
    .await
    .unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    match result.result.unwrap() {
        massa_proto_rs::massa::api::v1::send_endorsements_response::Result::Error(err) => {
            assert!(err.message.contains("failed to deserialize endorsement"))
        }
        _ => panic!("should be error"),
    }

    stop_handle.stop();
}

#[tokio::test]
async fn send_blocks() {
    let addr: SocketAddr = "[::]:4025".parse().unwrap();
    let mut public_server = grpc_public_service(&addr);
    let config = public_server.grpc_config.clone();
    // let keypair = KeyPair::generate(0).unwrap();
    let mut protocol_ctrl = Box::new(MockProtocolController::new());
    protocol_ctrl
        .expect_clone_box()
        .returning(|| Box::new(MockProtocolController::new()));

    let mut consensus_ctrl = Box::new(MockConsensusController::new());
    consensus_ctrl
        .expect_clone_box()
        .returning(|| Box::new(MockConsensusController::new()));

    public_server.protocol_controller = protocol_ctrl;
    public_server.consensus_controller = consensus_ctrl;

    let (_tx, rx) = tokio::sync::mpsc::channel(10);
    let request_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    let stop_handle = public_server.serve(&config).await.unwrap();

    // let secured_block: SecureShareBlock = block
    //     .new_verifiable(BlockSerializer::new(), &keypair)
    //     .unwrap();

    let mut public_client = PublicServiceClient::connect(format!(
        "grpc://localhost:{}",
        addr.to_string().split(':').last().unwrap()
    ))
    .await
    .unwrap();

    let resp_stream = public_client.send_blocks(request_stream).await;
    assert!(resp_stream.unwrap_err().message().contains("not available"));

    // tx.send(SendBlocksRequest {
    //     block: secured_block.serialized_data.clone(),
    // })
    // .await
    // .unwrap();

    // let result = tokio::time::timeout(Duration::from_secs(5), resp_stream.next())
    //     .await
    //     .unwrap()
    //     .unwrap()
    //     .unwrap();

    // match result.result.unwrap() {
    //     massa_proto_rs::massa::api::v1::send_blocks_response::Result::Error(err) => {
    //         assert!(err.message.contains("not available"))
    //     }
    //     _ => panic!("should be error"),
    // }

    // let endo1 = Endorsement::new_verifiable(
    //     Endorsement {
    //         slot: Slot::new(1, 0),
    //         index: 0,
    //         endorsed_block: BlockId::generate_from_hash(
    //             Hash::from_bs58_check("bq1NsaCBAfseMKSjNBYLhpK7M5eeef2m277MYS2P2k424GaDf").unwrap(),
    //         ),
    //     },
    //     EndorsementSerializer::new(),
    //     &keypair,
    // )
    // .unwrap();
    // let endo2 = Endorsement::new_verifiable(
    //     Endorsement {
    //         slot: Slot::new(1, 0),
    //         index: ENDORSEMENT_COUNT - 1,
    //         endorsed_block: BlockId::generate_from_hash(
    //             Hash::from_bs58_check("bq1NsaCBAfseMKSjNBYLhpK7M5eeef2m277MYS2P2k424GaDf").unwrap(),
    //         ),
    //     },
    //     EndorsementSerializer::new(),
    //     &keypair,
    // )
    // .unwrap();

    // create block header
    // let orig_header = BlockHeader::new_verifiable(
    //     BlockHeader {
    //         current_version: 0,
    //         announced_version: None,
    //         slot: Slot::new(1, 0),
    //         parents,
    //         operation_merkle_root: Hash::compute_from("mno".as_bytes()),
    //         endorsements: vec![],
    //         denunciations: Vec::new(), // FIXME
    //     },
    //     BlockHeaderSerializer::new(),
    //     &keypair,
    // )
    // .unwrap();

    // // create block
    // let orig_block = Block {
    //     header: orig_header,
    //     operations: Default::default(),
    // };

    // let secured_block: SecureShareBlock =
    //     Block::new_verifiable(orig_block, BlockSerializer::new(), &keypair).unwrap();

    // secured_block.content.header.verify_signature().unwrap();

    // secured_block.verify_signature().unwrap();

    stop_handle.stop();
}
