use super::tools::*;
use crate::start_storage;
use std::collections::HashMap;

#[tokio::test]
async fn test_add() {
    let (cfg, serialization_context) = get_test_config();
    let (command_sender, _manager) = start_storage(cfg, serialization_context).unwrap();
    command_sender.clear().await.unwrap(); // make sur that the db is empty
    assert_eq!(0, command_sender.len().await.unwrap());
    let hash = get_test_block_id();
    let block = get_test_block();
    command_sender.add_block(hash, block).await.unwrap();
    assert!(command_sender.contains(hash).await.unwrap());
    assert_eq!(1, command_sender.len().await.unwrap());
    command_sender.clear().await.unwrap();
}

#[tokio::test]
async fn test_add_multiple() {
    let (cfg, serialization_context) = get_test_config();
    let (command_sender, _manager) = start_storage(cfg, serialization_context).unwrap();
    command_sender.clear().await.unwrap(); // make sur that the db is empty
    let hash = get_test_block_id();
    let block = get_test_block();
    let mut map = HashMap::new();
    map.insert(hash, block);
    command_sender.add_block_batch(map).await.unwrap();
    assert!(command_sender.contains(hash).await.unwrap());
    command_sender.clear().await.unwrap();
}

#[tokio::test]
async fn test_get() {
    let (cfg, serialization_context) = get_test_config();
    let (command_sender, _manager) = start_storage(cfg, serialization_context.clone()).unwrap();
    command_sender.clear().await.unwrap(); // make sure that the db is empty
    assert_eq!(0, command_sender.len().await.unwrap());
    let hash = get_test_block_id();
    let block = get_test_block();
    command_sender.add_block(hash, block.clone()).await.unwrap();
    let retrived = command_sender.get_block(hash).await.unwrap().unwrap();

    assert_eq!(
        retrived
            .header
            .content
            .compute_hash(&serialization_context)
            .unwrap(),
        block
            .header
            .content
            .compute_hash(&serialization_context)
            .unwrap()
    );

    assert!(command_sender
        .get_block(get_another_test_block_id())
        .await
        .unwrap()
        .is_none());
    command_sender.clear().await.unwrap();
}

#[tokio::test]
async fn test_contains() {
    let (cfg, serialization_context) = get_test_config();
    let (command_sender, _manager) = start_storage(cfg, serialization_context).unwrap();
    command_sender.clear().await.unwrap(); // make sur that the db is empty
                                           //test in an empty db that the contains return false.
    assert!(!command_sender
        .contains(get_another_test_block_id())
        .await
        .unwrap());

    assert_eq!(0, command_sender.len().await.unwrap());
    let hash = get_test_block_id();
    let block = get_test_block();
    command_sender.add_block(hash, block.clone()).await.unwrap();

    //test the block is predent in db
    assert!(command_sender.contains(hash).await.unwrap());

    //test that another block isn't present
    assert!(!command_sender
        .contains(get_another_test_block_id())
        .await
        .unwrap());
}
