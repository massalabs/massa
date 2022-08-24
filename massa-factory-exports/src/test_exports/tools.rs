use massa_hash::Hash;
use massa_models::{
    wrapped::WrappedContent, Block, BlockHeader, BlockHeaderSerializer, BlockSerializer, Slot,
    WrappedBlock,
};
use massa_signature::KeyPair;

/// Create an empty block for testing. Can be used to generate genesis blocks.
pub fn create_empty_block(keypair: &KeyPair, slot: &Slot) -> WrappedBlock {
    let header = BlockHeader::new_wrapped(
        BlockHeader {
            slot: *slot,
            parents: Vec::new(),
            operation_merkle_root: Hash::compute_from(&Vec::new()),
            endorsements: Vec::new(),
        },
        BlockHeaderSerializer::new(),
        keypair,
    )
    .unwrap();

    Block::new_wrapped(
        Block {
            header,
            operations: Default::default(),
        },
        BlockSerializer::new(),
        keypair,
    )
    .unwrap()
}
