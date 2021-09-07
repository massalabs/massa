use crypto::{
    hash::Hash,
    signature::{PrivateKey, PublicKey},
};
use models::{Block, BlockHeader, BlockHeaderContent, BlockId, Endorsement, Operation, Slot};

struct BlockFactory {
    thread_count: u8,
    best_parents: Vec<BlockId>,
    creator_priv_key: PrivateKey,
    slot: Slot,
    operation_merkle_root: Hash,
    endorsements: Vec<Endorsement>,
    operations: Vec<Operation>,
}

impl BlockFactory {
    fn start_block_factory() -> BlockFactory {
        todo!()
    }

    fn create_block(&self) -> (BlockId, Block) {
        let public_key = crypto::derive_public_key(&self.creator_priv_key);
        let (hash, header) = BlockHeader::new_signed(
            &self.creator_priv_key,
            BlockHeaderContent {
                creator: public_key,
                slot: self.slot,
                parents: self.best_parents.clone(),
                operation_merkle_root: self.operation_merkle_root,
                endorsements: self.endorsements.clone(),
            },
        )
        .unwrap();

        let block = Block {
            header,
            operations: self.operations.clone(),
        };

        (hash, block)
    }

    fn set_slot(mut self, slot: Slot) -> BlockFactory {
        todo!()
    }

    fn set_parents(mut self, parents: Vec<BlockId>) -> BlockFactory {
        todo!()
    }

    fn set_creator(mut self, creator: PublicKey) -> BlockFactory {
        todo!()
    }
    fn set_endorsements(mut self, endorsements: Vec<Endorsement>) -> BlockFactory {
        todo!()
    }
    fn set_operations(mut self, operations: Vec<Operation>) -> BlockFactory {
        todo!()
    }
}
