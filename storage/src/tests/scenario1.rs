use super::super::{config::StorageConfig, storage_controller::start_storage_controller};
use super::tools;
use crate::storage_controller::StorageCommandSender;
use crypto::hash::Hash;
use models::slot::Slot;

#[tokio::test]
async fn test_max_nb_blocks() {
    let filename = "target/tests/max_nb_block.db";
    let config = StorageConfig {
        /// Max number of bytes we want to store
        max_stored_blocks: 5,
        /// path to db
        path: filename.to_string(), //in target to be ignored by git and different file between test.
        cache_capacity: 256,  //little to force flush cache
        flush_every_ms: None, //defaut
    };

    let (storage, manager) = start_storage_controller(config).unwrap();
    storage.clear().await.unwrap(); // make sur that the db is empty
    assert_eq!(0, storage.len().await.unwrap());
    //write 6 block. 5 must be in db after. The (1,0) must be removed.
    add_block(Slot::new(2, 1), &storage).await;
    assert_eq!(1, storage.len().await.unwrap());
    add_block(Slot::new(1, 1), &storage).await;
    assert_eq!(2, storage.len().await.unwrap());
    add_block(Slot::new(3, 0), &storage).await;
    assert_eq!(3, storage.len().await.unwrap());
    add_block(Slot::new(1, 0), &storage).await;
    assert_eq!(4, storage.len().await.unwrap());
    add_block(Slot::new(3, 1), &storage).await;
    assert_eq!(5, storage.len().await.unwrap());
    add_block(Slot::new(4, 0), &storage).await;
    assert_eq!(5, storage.len().await.unwrap());
    let result = storage
        .get_slot_range(Slot::new(0, 0), Slot::new(1, 1))
        .await
        .unwrap();
    assert_eq!(0, result.len());
    add_block(Slot::new(4, 1), &storage).await;
    let result = storage
        .get_slot_range(Slot::new(0, 0), Slot::new(2, 1))
        .await
        .unwrap();
    assert_eq!(0, result.len());

    manager.stop().await.unwrap();
}

#[tokio::test]
async fn test_get_slot_range() {
    let filename = "target/tests/get_slot_range.db";
    let config = StorageConfig {
        /// Max number of bytes we want to store
        max_stored_blocks: 10,
        /// path to db
        path: filename.to_string(), //in target to be ignored by git and different file between test.
        cache_capacity: 256,  //little to force flush cache
        flush_every_ms: None, //defaut
    };

    let (storage, manager) = start_storage_controller(config).unwrap();
    storage.clear().await.unwrap(); // make sur that the db is empty
    assert_eq!(0, storage.len().await.unwrap());
    //add block in this order depending on there periode and thread
    add_block(Slot::new(2, 1), &storage).await;
    add_block(Slot::new(1, 0), &storage).await;
    add_block(Slot::new(1, 1), &storage).await;
    add_block(Slot::new(3, 0), &storage).await;
    add_block(Slot::new(3, 1), &storage).await;
    add_block(Slot::new(4, 0), &storage).await;
    assert_eq!(6, storage.len().await.unwrap());

    // search for (1,2) (3,1)
    let result = storage
        .get_slot_range(Slot::new(1, 1), Slot::new(3, 1))
        .await
        .unwrap();
    //println!("result:{:#?}", result);
    assert!(result.contains_key(&Hash::hash(b"(period: 1, thread: 1)")));
    assert!(result.contains_key(&Hash::hash(b"(period: 2, thread: 1)")));
    assert!(result.contains_key(&Hash::hash(b"(period: 3, thread: 0)")));
    assert!(!result.contains_key(&Hash::hash(b"(period: 3, thread: 1)")));
    assert!(!result.contains_key(&Hash::hash(b"(period: 1, thread: 0)")));
    assert!(!result.contains_key(&Hash::hash(b"(period: 2, thread: 0)")));

    //range too low
    let result = storage
        .get_slot_range(Slot::new(0, 0), Slot::new(1, 0))
        .await
        .unwrap();
    assert_eq!(0, result.len());
    //range too after
    let result = storage
        .get_slot_range(Slot::new(4, 1), Slot::new(6, 1))
        .await
        .unwrap();
    //    println!("result:{:?}", result);
    assert_eq!(0, result.len());
    //unique range be after
    let result = storage
        .get_slot_range(Slot::new(1, 1), Slot::new(1, 1))
        .await
        .unwrap();
    assert_eq!(0, result.len());
    //bad range
    let result = storage
        .get_slot_range(Slot::new(3, 1), Slot::new(1, 1))
        .await
        .unwrap();
    assert_eq!(0, result.len());

    //unique range inf out
    let result = storage
        .get_slot_range(Slot::new(0, 0), Slot::new(1, 1))
        .await
        .unwrap();
    assert!(result.contains_key(&Hash::hash(b"(period: 1, thread: 0)")));
    //unique range sup out
    let result = storage
        .get_slot_range(Slot::new(4, 0), Slot::new(5, 1))
        .await
        .unwrap();
    assert!(result.contains_key(&Hash::hash(b"(period: 4, thread: 0)")));

    manager.stop().await.unwrap();
}

async fn add_block(slot: Slot, storage: &StorageCommandSender) {
    let mut block = tools::get_test_block();
    block.header.slot = slot;
    let hash = Hash::hash(format!("{}", slot).as_bytes());
    storage.add_block(hash, block).await.unwrap();
}
