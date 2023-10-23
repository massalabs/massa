use std::{time::Duration, vec};

use crate::start_consensus_worker;
use massa_channel::MassaChannel;
use massa_consensus_exports::{
    ConsensusBroadcasts, ConsensusChannels, ConsensusConfig, ConsensusController,
};
use massa_execution_exports::MockExecutionController;
use massa_hash::Hash;
use massa_metrics::MassaMetrics;
use massa_models::{
    block::{Block, BlockSerializer, SecureShareBlock},
    block_header::{BlockHeader, BlockHeaderSerializer},
    block_id::BlockId,
    config::THREAD_COUNT,
    secure_share::SecureShareContent,
    slot::Slot,
};
use massa_pool_exports::MockPoolController;
use massa_pos_exports::MockSelectorController;
use massa_protocol_exports::MockProtocolController;
use massa_signature::KeyPair;
use massa_storage::Storage;

pub fn consensus_test<F>(
    cfg: ConsensusConfig,
    execution_controller: Box<MockExecutionController>,
    pool_controller: Box<MockPoolController>,
    selector_controller: Box<MockSelectorController>,
    test: F,
) where
    F: FnOnce(Box<dyn ConsensusController>),
{
    let storage: Storage = Storage::create_root();
    // mock protocol
    let mut protocol_controller = Box::new(MockProtocolController::new());
    //TODO: Test better here for example number of times
    protocol_controller
        .expect_integrated_block()
        .returning(|_, _| Ok(()));
    protocol_controller
        .expect_send_wishlist_delta()
        .returning(|_, _| Ok(()));
    protocol_controller
        .expect_notify_block_attack()
        .returning(|_| Ok(()));
    // launch consensus controller
    let (consensus_event_sender, _) = MassaChannel::new(String::from("consensus_event"), Some(10));

    // All API channels
    let (block_sender, _block_receiver) = tokio::sync::broadcast::channel(10);
    let (block_header_sender, _block_header_receiver) = tokio::sync::broadcast::channel(10);
    let (filled_block_sender, _filled_block_receiver) = tokio::sync::broadcast::channel(10);
    let (consensus_controller, mut consensus_manager) = start_consensus_worker(
        cfg.clone(),
        ConsensusChannels {
            broadcasts: ConsensusBroadcasts {
                block_sender,
                block_header_sender,
                filled_block_sender,
            },
            controller_event_tx: consensus_event_sender,
            execution_controller,
            protocol_controller,
            pool_controller,
            selector_controller,
        },
        None,
        storage.clone(),
        MassaMetrics::new(
            false,
            "0.0.0.0:9898".parse().unwrap(),
            THREAD_COUNT,
            Duration::from_secs(1),
        )
        .0,
    );

    // Call test func.
    test(consensus_controller);
    // stop controller while ignoring all commands
    consensus_manager.stop();
}

// returns hash and resulting discarded blocks
pub fn create_block(slot: Slot, best_parents: Vec<BlockId>, creator: &KeyPair) -> SecureShareBlock {
    create_block_with_merkle_root(
        Hash::compute_from("default_val".as_bytes()),
        slot,
        best_parents,
        creator,
    )
}

// returns hash and resulting discarded blocks
pub fn create_block_with_merkle_root(
    operation_merkle_root: Hash,
    slot: Slot,
    best_parents: Vec<BlockId>,
    creator: &KeyPair,
) -> SecureShareBlock {
    let header = BlockHeader::new_verifiable(
        BlockHeader {
            current_version: 0,
            announced_version: None,
            denunciations: vec![],
            slot,
            parents: best_parents,
            operation_merkle_root,
            endorsements: Vec::new(),
        },
        BlockHeaderSerializer::new(),
        creator,
    )
    .unwrap();

    Block::new_verifiable(
        Block {
            header,
            operations: Default::default(),
        },
        BlockSerializer::new(),
        creator,
    )
    .unwrap()
}

pub fn register_block(
    consensus_controller: &Box<dyn ConsensusController>,
    block: SecureShareBlock,
    mut storage: Storage,
) {
    storage.store_block(block.clone());
    consensus_controller.register_block(
        block.id,
        block.content.header.content.slot,
        storage.clone(),
        false,
    );
}
