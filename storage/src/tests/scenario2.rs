use std::collections::HashMap;

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
async fn test_add_multiple() {
    let cfg = get_test_config("target/test/add_multiple".into());
    let (command_sender, _manager) = start_storage_controller(cfg).unwrap();
    command_sender.reset().await.unwrap(); // make sur that the db is empty
    let hash = get_test_hash();
    let block = get_test_block();
    let mut map = HashMap::new();
    map.insert(hash, block);
    command_sender.add_multiple_blocks(map).await.unwrap();
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
    command_sender.add_block(hash, block.clone()).await.unwrap();
    let retrived = command_sender.get_block(hash).await.unwrap().unwrap();

    assert_eq!(
        retrived.header.compute_hash().unwrap(),
        block.header.compute_hash().unwrap()
    );

    assert!(command_sender
        .get_block(get_another_test_hash())
        .await
        .unwrap()
        .is_none());
    command_sender.reset().await.unwrap();
}

#[tokio::test]
async fn test_contains() {
    let cfg = get_test_config("target/test/contains".into());
    let (command_sender, _manager) = start_storage_controller(cfg).unwrap();
    command_sender.reset().await.unwrap(); // make sur that the db is empty
    let hash = get_test_hash();
    let block = get_test_block();
    command_sender.add_block(hash, block.clone()).await.unwrap();

    assert!(command_sender.contains(hash).await.unwrap());

    assert!(!command_sender
        .contains(get_another_test_hash())
        .await
        .unwrap())
}
