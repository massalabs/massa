// Copyright (c) 2021 MASSA LABS <info@massa.net>

use super::tools::*;
use models::Amount;
use serial_test::serial;
use std::str::FromStr;

#[tokio::test]
#[serial]
async fn test_add() {
    let cfg = get_test_config();
    storage_test(cfg, async move |command_sender| {
        assert_eq!(0, command_sender.len().await.unwrap());
        let hash = get_test_block_id();
        let block = get_test_block();
        let op_ids = get_operation_set(&block.operations);
        command_sender.add_block(hash, block, op_ids).await.unwrap();
        assert!(command_sender.contains(hash).await.unwrap());
        assert_eq!(1, command_sender.len().await.unwrap());
    })
    .await;
}

#[tokio::test]
#[serial]
async fn test_find_operation() {
    let cfg = get_test_config();
    storage_test(cfg, async move |command_sender| {
        assert_eq!(0, command_sender.len().await.unwrap());
        let (block, id, op) = get_block_with_op();
        let op_ids = get_operation_set(&block.operations);
        command_sender.add_block(id, block, op_ids).await.unwrap();
        let (out_idx, out_final) = command_sender
            .get_operations(vec![op].into_iter().collect())
            .await
            .unwrap()[&op]
            .in_blocks[&id];
        assert_eq!((out_idx, out_final), (0, true));
        let mut op2 = create_operation();
        op2.content.fee = Amount::from_str("42").unwrap();
        let id2 = op2.get_operation_id().unwrap();
        assert!(!command_sender
            .get_operations(vec![id2].into_iter().collect())
            .await
            .unwrap()
            .contains_key(&id2));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn test_get() {
    // stderrlog::new()
    //     .verbosity(2)
    //     .timestamp(stderrlog::Timestamp::Millisecond)
    //     .init()
    //     .unwrap();
    let cfg = get_test_config();
    storage_test(cfg, async move |command_sender| {
        assert_eq!(0, command_sender.len().await.unwrap());
        let hash = get_test_block_id();
        let block = get_test_block();
        let op_ids = get_operation_set(&block.operations);
        command_sender
            .add_block(hash, block.clone(), op_ids)
            .await
            .unwrap();
        let retrieved = command_sender.get_block(hash).await.unwrap().unwrap();

        assert_eq!(
            retrieved.header.content.compute_hash().unwrap(),
            block.header.content.compute_hash().unwrap()
        );

        assert!(command_sender
            .get_block(get_another_test_block_id())
            .await
            .unwrap()
            .is_none());
    })
    .await;
}

#[tokio::test]
#[serial]
async fn test_contains() {
    let cfg = get_test_config();
    storage_test(cfg, async move |command_sender| {
        //test in an empty db that the contains return false.
        assert!(!command_sender
            .contains(get_another_test_block_id())
            .await
            .unwrap());

        assert_eq!(0, command_sender.len().await.unwrap());
        let hash = get_test_block_id();
        let block = get_test_block();
        let op_ids = get_operation_set(&block.operations);
        command_sender
            .add_block(hash, block.clone(), op_ids)
            .await
            .unwrap();

        //test the block is present in db
        assert!(command_sender.contains(hash).await.unwrap());

        //test that another block isn't present
        assert!(!command_sender
            .contains(get_another_test_block_id())
            .await
            .unwrap());
    })
    .await;
}
