// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This is a factory that can be used in consensus test
//! but at it was introduced quite late in the development process
//! it has only be used in scenarios basic

use super::tools::{validate_notpropagate_block, validate_propagate_block};
use massa_hash::Hash;
use massa_models::{
    wrapped::{Id, WrappedContent},
    Block, BlockHeader, BlockHeaderSerializer, BlockId, BlockSerializer, Slot, WrappedBlock,
    WrappedEndorsement, WrappedOperation,
};
use massa_protocol_exports::test_exports::MockProtocolController;
use massa_signature::KeyPair;
use massa_storage::Storage;

pub struct BlockFactory {
    pub best_parents: Vec<BlockId>,
    pub creator_keypair: KeyPair,
    pub slot: Slot,
    pub endorsements: Vec<WrappedEndorsement>,
    pub operations: Vec<WrappedOperation>,
    pub protocol_controller: MockProtocolController,
}

impl BlockFactory {
    pub fn start_block_factory(
        genesis: Vec<BlockId>,
        protocol_controller: MockProtocolController,
    ) -> BlockFactory {
        BlockFactory {
            best_parents: genesis,
            creator_keypair: KeyPair::generate(),
            slot: Slot::new(1, 0),
            endorsements: Vec::new(),
            operations: Vec::new(),
            protocol_controller,
        }
    }

    pub async fn create_and_receive_block(&mut self, valid: bool) -> WrappedBlock {
        let header = BlockHeader::new_wrapped(
            BlockHeader {
                slot: self.slot,
                parents: self.best_parents.clone(),
                operation_merkle_root: Hash::compute_from(
                    &self
                        .operations
                        .iter()
                        .flat_map(|op| op.id.hash().into_bytes())
                        .collect::<Vec<_>>()[..],
                ),
                endorsements: self.endorsements.clone(),
            },
            BlockHeaderSerializer::new(),
            &self.creator_keypair,
        )
        .unwrap();

        let block = Block::new_wrapped(
            Block {
                header,
                operations: self
                    .operations
                    .clone()
                    .into_iter()
                    .map(|op| op.id)
                    .collect(),
            },
            BlockSerializer::new(),
            &self.creator_keypair,
        )
        .unwrap();

        let mut storage = Storage::default();
        let id = block.id;
        let slot = block.content.header.content.slot;
        storage.store_block(block.clone());

        self.protocol_controller
            .receive_block(id, slot, storage)
            .await;
        if valid {
            // Assert that the block is propagated.
            validate_propagate_block(&mut self.protocol_controller, id, 2000).await;
        } else {
            // Assert that the the block is not propagated.
            validate_notpropagate_block(&mut self.protocol_controller, id, 500).await;
        }
        block
    }

    pub fn sign_header(&self, header: BlockHeader) -> WrappedBlock {
        let header =
            BlockHeader::new_wrapped(header, BlockHeaderSerializer::new(), &self.creator_keypair)
                .unwrap();

        Block::new_wrapped(
            Block {
                header,
                operations: self
                    .operations
                    .clone()
                    .into_iter()
                    .map(|op| op.id)
                    .collect(),
            },
            BlockSerializer::new(),
            &self.creator_keypair,
        )
        .unwrap()
    }

    pub async fn receive_block(
        &mut self,
        valid: bool,
        block_id: BlockId,
        slot: Slot,
        storage: Storage,
    ) {
        self.protocol_controller
            .receive_block(block_id, slot, storage)
            .await;
        if valid {
            // Assert that the block is propagated.
            validate_propagate_block(&mut self.protocol_controller, block_id, 2000).await;
        } else {
            // Assert that the the block is not propagated.
            validate_notpropagate_block(&mut self.protocol_controller, block_id, 500).await;
        }
    }

    pub fn take_protocol_controller(self) -> MockProtocolController {
        self.protocol_controller
    }
}
