use crate::Storage;
use massa_factory_exports::test_exports::create_empty_block;
use massa_models::{prehash::PreHashSet, slot::Slot};
use massa_signature::KeyPair;

#[test]
/// Store a block and retrieve it.
fn test_basic_insert() {
    let mut storage = Storage::create_root();
    let block = create_empty_block(&KeyPair::generate(0).unwrap(), &Slot::new(0, 0));

    storage.store_block(block.clone());
    let blocks = storage.read_blocks();
    let stored_block = blocks.get(&block.id).unwrap();
    assert_eq!(stored_block.id, block.id);
    assert_eq!(stored_block.serialized_data, block.serialized_data);
}

#[test]
/// Test double insert of the same block.
/// We expect that it's stored only one time
fn test_double_insert() {
    let mut storage = Storage::create_root();
    let block = create_empty_block(&KeyPair::generate(0).unwrap(), &Slot::new(0, 0));

    storage.store_block(block.clone());
    storage.store_block(block.clone());
    {
        let blocks = storage.read_blocks();
        blocks.get(&block.id).unwrap();
    };
    let mut ids = PreHashSet::default();
    ids.insert(block.id);
    storage.drop_block_refs(&ids);
    {
        let blocks = storage.read_blocks();
        assert!(blocks.get(&block.id).is_none());
    };
}
