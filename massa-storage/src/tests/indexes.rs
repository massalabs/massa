use crate::Storage;
use massa_factory_exports::test_exports::create_empty_block;
use massa_models::{address::Address, slot::Slot};
use massa_signature::KeyPair;

#[test]
fn test_block_index_slot() {
    let mut storage = Storage::create_root();
    let slot = Slot::new(0, 0);
    let block = create_empty_block(&KeyPair::generate(0).unwrap(), &slot);

    storage.store_block(block.clone());
    let blocks = storage.read_blocks();
    let stored_blocks = blocks.get_blocks_by_slot(&slot).unwrap();
    assert_eq!(stored_blocks.len(), 1);
    assert_eq!(stored_blocks.get(&block.id).unwrap(), &block.id);
}

#[test]
fn test_block_index_by_creator() {
    let mut storage = Storage::create_root();
    let slot = Slot::new(0, 0);
    let keypair = KeyPair::generate(0).unwrap();
    let block = create_empty_block(&keypair, &slot);

    storage.store_block(block.clone());
    let blocks = storage.read_blocks();
    let stored_blocks = blocks
        .get_blocks_created_by(&Address::from_public_key(&keypair.get_public_key()))
        .unwrap();
    assert_eq!(stored_blocks.len(), 1);
    assert_eq!(stored_blocks.get(&block.id).unwrap(), &block.id);
}

#[test]
fn test_block_fail_find() {
    let mut storage = Storage::create_root();
    let slot = Slot::new(0, 0);
    let keypair = KeyPair::generate(0).unwrap();
    let keypair2 = KeyPair::generate(0).unwrap();
    let block = create_empty_block(&keypair, &slot);

    storage.store_block(block);
    let blocks = storage.read_blocks();
    assert!(blocks
        .get_blocks_created_by(&Address::from_public_key(&keypair2.get_public_key()))
        .is_none());
}
