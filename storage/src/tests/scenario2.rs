use std::collections::HashMap;

use crate::storage_controller::start_storage_controller;

use super::tools::*;

#[tokio::test]
async fn test_add() {
    let cfg = get_test_config();
    let (command_sender, _manager) = start_storage_controller(cfg).unwrap();
    command_sender.clear().await.unwrap(); // make sur that the db is empty
    assert_eq!(0, command_sender.len().await.unwrap());
    let hash = get_test_hash();
    let block = get_test_block();
    command_sender.add_block(hash, block).await.unwrap();
    assert!(command_sender.contains(hash).await.unwrap());
    assert_eq!(1, command_sender.len().await.unwrap());
    command_sender.clear().await.unwrap();
}

#[tokio::test]
async fn test_add_multiple() {
    let cfg = get_test_config();
    let (command_sender, _manager) = start_storage_controller(cfg).unwrap();
    command_sender.clear().await.unwrap(); // make sur that the db is empty
    let hash = get_test_hash();
    let block = get_test_block();
    let mut map = HashMap::new();
    map.insert(hash, block);
    command_sender.add_multiple_blocks(map).await.unwrap();
    assert!(command_sender.contains(hash).await.unwrap());
    command_sender.clear().await.unwrap();
}

#[tokio::test]
async fn test_get() {
    let cfg = get_test_config();
    let (command_sender, _manager) = start_storage_controller(cfg).unwrap();
    command_sender.clear().await.unwrap(); // make sur that the db is empty
    assert_eq!(0, command_sender.len().await.unwrap());
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
    command_sender.clear().await.unwrap();
}

#[tokio::test]
async fn test_contains() {
    let cfg = get_test_config();
    let (command_sender, _manager) = start_storage_controller(cfg).unwrap();
    command_sender.clear().await.unwrap(); // make sur that the db is empty
    assert_eq!(0, command_sender.len().await.unwrap());
    let hash = get_test_hash();
    let block = get_test_block();
    command_sender.add_block(hash, block.clone()).await.unwrap();

    assert!(command_sender.contains(hash).await.unwrap());

    assert!(!command_sender
        .contains(get_another_test_hash())
        .await
        .unwrap())
}
