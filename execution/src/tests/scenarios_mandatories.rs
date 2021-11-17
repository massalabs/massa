// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::{start_controller, ExecutionConfig};
use serial_test::serial;

#[tokio::test]
#[serial]
async fn test_basic_transaction() {
    assert!(start_controller(ExecutionConfig {}).await.is_ok());
}
