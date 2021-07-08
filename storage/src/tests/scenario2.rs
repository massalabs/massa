use crate::storage_controller::start_storage_controller;

use super::tools::*;

#[tokio::test]
async fn test_add() {
    let cfg = get_test_config("target/test/add".into());
    let (command_sender, _manager) = start_storage_controller(cfg).unwrap();
    command_sender.reset().await.unwrap(); // make sur that the db is empty
    let hash = get_test_hash();
    let block = get_test_block();
    command_sender.add_block(hash, block).await.unwrap();
    assert!(command_sender.contains(hash).await.unwrap());
    command_sender.reset().await.unwrap();
}

#[tokio::test]
async fn test_get() {
    let cfg = get_test_config("target/test/get".into());
    let (command_sender, _manager) = start_storage_controller(cfg).unwrap();
    command_sender.reset().await.unwrap(); // make sur that the db is empty
    let hash = get_test_hash();
    let block = get_test_block();
    command_sender.add_block(hash, block).await.unwrap();
    assert!(command_sender.contains(hash).await.unwrap());
    assert!(!command_sender
        .contains(get_another_test_hash())
        .await
        .unwrap());
    command_sender.reset().await.unwrap();
}
