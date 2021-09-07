use crypto::{
    hash::Hash,
    signature::{PrivateKey, PublicKey},
};
use models::{Block, BlockHeader, BlockHeaderContent, BlockId, Endorsement, Operation, Slot};

struct BlockFactory {
    best_parents: Vec<BlockId>,
    creator_priv_key: PrivateKey,
    slot: Slot,
    endorsements: Vec<Endorsement>,
    operations: Vec<Operation>,
}

impl BlockFactory {
    fn start_block_factory(genesis: Vec<BlockId>) -> BlockFactory {
        BlockFactory {
            best_parents: genesis,
            creator_priv_key: crypto::generate_random_private_key(),
            slot: Slot::new(1, 0),
            endorsements: Vec::new(),
            operations: Vec::new(),
        }
    }

    fn create_block(&self) -> (BlockId, Block) {
        let public_key = crypto::derive_public_key(&self.creator_priv_key);
        let (hash, header) = BlockHeader::new_signed(
            &self.creator_priv_key,
            BlockHeaderContent {
                creator: public_key,
                slot: self.slot,
                parents: self.best_parents.clone(),
                operation_merkle_root: Hash::hash(
                    &self
                        .operations
                        .iter()
                        .map(|op| op.get_operation_id().unwrap().to_bytes().clone())
                        .flatten()
                        .collect::<Vec<_>>()[..],
                ),
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
        self.slot = slot;
        self
    }

    fn set_parents(mut self, parents: Vec<BlockId>) -> BlockFactory {
        self.best_parents = parents;
        self
    }

    fn set_creator(mut self, creator: PrivateKey) -> BlockFactory {
        self.creator_priv_key = creator;
        self
    }
    fn set_endorsements(mut self, endorsements: Vec<Endorsement>) -> BlockFactory {
        self.endorsements = endorsements;
        self
    }
    fn set_operations(mut self, operations: Vec<Operation>) -> BlockFactory {
        self.operations = operations;
        self
    }
}
