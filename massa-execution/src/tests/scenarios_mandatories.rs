// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::{start_controller, ExecutionConfig};
use serial_test::serial;

#[tokio::test]
#[serial]
async fn test_execution_basic() {
    assert!(start_controller(ExecutionConfig {}, 2).await.is_ok());
}

#[tokio::test]
#[serial]
async fn test_execution_shutdown() {
    let (_command_sender, event_receiver, manager) = start_controller(ExecutionConfig {}, 2)
        .await
        .expect("Failed to start execution.");
    manager
        .stop(event_receiver)
        .await
        .expect("Failed to stop execution.");
}

#[tokio::test]
#[serial]
async fn test_sending_command() {
    let (mut command_sender, event_receiver, manager) = start_controller(ExecutionConfig {}, 2)
        .await
        .expect("Failed to start execution.");
    command_sender
        .update_blockclique(Default::default(), Default::default())
        .await
        .expect("Failed to send command");
    manager
        .stop(event_receiver)
        .await
        .expect("Failed to stop execution.");
}
