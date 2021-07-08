use super::super::{config::StorageConfig, storage_controller::start_storage_controller};
use super::tools;
use crate::storage_controller::StorageCommandSender;
use crypto::hash::Hash;

#[tokio::test]
async fn test_get_slot_range() {
    let filename = "target/tests/get_slot_range.db";
    tokio::fs::remove_file(filename).await.unwrap_or(()); //error if deosn't exist
    let config = StorageConfig {
        /// Max number of bytes we want to store
        max_capacity: 0, //not used?
        /// path to db
        path: filename.to_string(), //in target to be ignored by git and different file between test.
        cache_capacity: 256,  //little to force flush cache
        flush_every_ms: None, //defaut
    };

    let (storage, manager) = start_storage_controller(config).unwrap();
    //add block in this order depending on there periode and thread
    add_block(2, 1, &storage).await;
    add_block(1, 0, &storage).await;
    add_block(1, 1, &storage).await;
    add_block(3, 0, &storage).await;
    add_block(3, 1, &storage).await;
    add_block(4, 0, &storage).await;

    // search for (1,2) (3,1)
    let result = storage.get_slot_range((1, 1), (3, 1)).await.unwrap();
    //println!("result:{:#?}", result);
    assert!(result.contains_key(&Hash::hash(b"(1,1)")));
    assert!(result.contains_key(&Hash::hash(b"(2,1)")));
    assert!(result.contains_key(&Hash::hash(b"(3,0)")));
    assert!(!result.contains_key(&Hash::hash(b"(3,1)")));
    assert!(!result.contains_key(&Hash::hash(b"(1,0)")));
    assert!(!result.contains_key(&Hash::hash(b"(2,0)")));

    //range too low
    let result = storage.get_slot_range((0, 0), (1, 0)).await.unwrap();
    assert_eq!(0, result.len());
    //range too after
    let result = storage.get_slot_range((4, 1), (6, 1)).await.unwrap();
    //    println!("result:{:?}", result);
    assert_eq!(0, result.len());
    //unique range be after
    let result = storage.get_slot_range((1, 1), (1, 1)).await.unwrap();
    assert_eq!(0, result.len());
    //bad range
    let result = storage.get_slot_range((3, 1), (1, 1)).await.unwrap();
    assert_eq!(0, result.len());

    //unique range inf out
    let result = storage.get_slot_range((0, 0), (1, 1)).await.unwrap();
    assert!(result.contains_key(&Hash::hash(b"(1,0)")));
    //unique range sup out
    let result = storage.get_slot_range((4, 0), (5, 1)).await.unwrap();
    assert!(result.contains_key(&Hash::hash(b"(4,0)")));

    manager.stop().await.unwrap();
}

async fn add_block(period: u64, slot: u8, storage: &StorageCommandSender) {
    let mut block = tools::get_test_block();
    block.header.period_number = period;
    block.header.thread_number = slot;
    let hash = Hash::hash(format!("({},{})", period, slot).as_bytes());
    storage.add_block(hash, block).await.unwrap();
}
