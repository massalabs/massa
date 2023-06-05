use crate::Storage;
use massa_factory_exports::test_exports::create_empty_block;
use massa_models::{prehash::PreHashSet, slot::Slot};
use massa_signature::KeyPair;

#[test]
fn test_clone() {
    let mut storage = Storage::create_root();
    let slot = Slot::new(0, 0);
    let block = create_empty_block(&KeyPair::generate(0).unwrap(), &slot);

    storage.store_block(block.clone());
    let storage2 = storage.clone();
    let block_ids = storage2.get_block_refs();
    let stored_block_id = block_ids.get(&block.id).unwrap();
    assert_eq!(stored_block_id, &block.id);
}

#[test]
fn test_clone_without_ref() {
    let mut storage = Storage::create_root();
    let slot = Slot::new(0, 0);
    let block = create_empty_block(&KeyPair::generate(0).unwrap(), &slot);

    storage.store_block(block.clone());
    let storage2 = storage.clone_without_refs();
    let blocks = storage2.get_block_refs();
    assert!(blocks.get(&block.id).is_none());
}

#[test]
fn test_retrieve_all_ref_dropped() {
    let mut storage = Storage::create_root();
    let slot = Slot::new(0, 0);
    let block = create_empty_block(&KeyPair::generate(0).unwrap(), &slot);

    storage.store_block(block.clone());
    let storage2 = storage.clone_without_refs();
    {
        let blocks = storage2.read_blocks();
        assert_eq!(
            blocks.get(&block.id).unwrap().serialized_data,
            block.serialized_data
        );
    };
    let mut ids = PreHashSet::default();
    ids.insert(block.id);
    storage.drop_block_refs(&ids);
    {
        let blocks = storage2.read_blocks();
        assert!(blocks.get(&block.id).is_none());
    };
    drop(storage);
}

#[test]
fn test_retrieve_all_ref_dropped_automatically() {
    let mut storage = Storage::create_root();
    let slot = Slot::new(0, 0);
    let block = create_empty_block(&KeyPair::generate(0).unwrap(), &slot);

    storage.store_block(block.clone());
    let storage2 = storage.clone_without_refs();
    {
        let blocks = storage2.read_blocks();
        assert_eq!(
            blocks.get(&block.id).unwrap().serialized_data,
            block.serialized_data
        );
    };
    drop(storage);
    {
        let blocks = storage2.read_blocks();
        assert!(blocks.get(&block.id).is_none());
    };
}
