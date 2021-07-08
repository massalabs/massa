use super::mock_protocol_controller::MockProtocolController;
use crate::{block_graph::BlockGraphExport, ledger::LedgerData, ConsensusConfig};
use communication::protocol::ProtocolCommand;
use crypto::{hash::Hash, signature::PrivateKey};
use models::{
    Address, Block, BlockHeader, BlockHeaderContent, BlockId, SerializationContext, Slot,
};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};
use storage::{StorageAccess, StorageConfig};
use tempfile::NamedTempFile;
use time::UTime;

//return true if another block has been seen
pub async fn validate_notpropagate_block(
    protocol_controller: &mut MockProtocolController,
    not_propagated: BlockId,
    timeout_ms: u64,
) -> bool {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::IntegratedBlock { block_id, .. } => Some(block_id),
            _ => None,
        })
        .await;
    match param {
        Some(block_id) => not_propagated != block_id,
        None => false,
    }
}

//return true if another block has been seen
pub async fn validate_notpropagate_block_in_list(
    protocol_controller: &mut MockProtocolController,
    not_propagated: &Vec<BlockId>,
    timeout_ms: u64,
) -> bool {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::IntegratedBlock { block_id, .. } => Some(block_id),
            _ => None,
        })
        .await;
    match param {
        Some(block_id) => !not_propagated.contains(&block_id),
        None => false,
    }
}

pub async fn validate_propagate_block_in_list(
    protocol_controller: &mut MockProtocolController,
    valid: &Vec<BlockId>,
    timeout_ms: u64,
) -> BlockId {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::IntegratedBlock { block_id, .. } => Some(block_id),
            _ => None,
        })
        .await;
    match param {
        Some(block_id) => {
            assert!(valid.contains(&block_id), "not the valid hash propagated");
            block_id
        }
        None => panic!("Hash not propagated."),
    }
}

pub async fn validate_ask_for_block(
    protocol_controller: &mut MockProtocolController,
    valid: BlockId,
    timeout_ms: u64,
) -> BlockId {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::WishlistDelta { new, .. } => Some(new),
            _ => None,
        })
        .await;
    match param {
        Some(new) => {
            assert!(new.contains(&valid), "not the valid hash asked for");
            assert_eq!(new.len(), 1);
            valid
        }
        None => panic!("Block not asked for before timeout."),
    }
}

pub async fn validate_wishlist(
    protocol_controller: &mut MockProtocolController,
    new: HashSet<BlockId>,
    remove: HashSet<BlockId>,
    timeout_ms: u64,
) {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::WishlistDelta { new, remove } => Some((new, remove)),
            _ => None,
        })
        .await;
    match param {
        Some((got_new, got_remove)) => {
            assert_eq!(new, got_new);
            assert_eq!(remove, got_remove);
        }
        None => panic!("Wishlist delta not sent for before timeout."),
    }
}

pub async fn validate_does_not_ask_for_block(
    protocol_controller: &mut MockProtocolController,
    hash: &BlockId,
    timeout_ms: u64,
) {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::WishlistDelta { new, .. } => Some(new),
            _ => None,
        })
        .await;
    match param {
        Some(new) => {
            if new.contains(hash) {
                panic!("unexpected ask for block {:?}", hash);
            }
        }
        None => {}
    }
}

pub async fn validate_propagate_block(
    protocol_controller: &mut MockProtocolController,
    valid_hash: BlockId,
    timeout_ms: u64,
) {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::IntegratedBlock { block_id, .. } => Some(block_id),
            _ => None,
        })
        .await;
    match param {
        Some(hash) => assert_eq!(valid_hash, hash, "not the valid hash propagated"),
        None => panic!("Block not propagated before timeout."),
    }
}

pub async fn validate_notify_block_attack_attempt(
    protocol_controller: &mut MockProtocolController,
    valid_hash: BlockId,
    timeout_ms: u64,
) {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::AttackBlockDetected(hash) => Some(hash),
            _ => None,
        })
        .await;
    match param {
        Some(hash) => assert_eq!(valid_hash, hash, "Attack attempt notified for wrong hash."),
        None => panic!("Attack attempt not notified before timeout."),
    }
}

pub fn start_storage(serialization_context: &SerializationContext) -> StorageAccess {
    let tempdir = tempfile::tempdir().expect("cannot create temp dir");
    let storage_config = StorageConfig {
        /// Max number of bytes we want to store
        max_stored_blocks: 50,
        /// path to db
        path: tempdir.path().to_path_buf(), //in target to be ignored by git and different file between test.
        cache_capacity: 256,  //little to force flush cache
        flush_interval: None, //defaut
        reset_at_startup: true,
    };
    let (storage_command_tx, _storage_manager) =
        storage::start_storage(storage_config, serialization_context.clone()).unwrap();
    storage_command_tx
}

pub async fn validate_block_found(
    protocol_controller: &mut MockProtocolController,
    valid_hash: &BlockId,
    timeout_ms: u64,
) {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::GetBlocksResults(results) => Some(results),
            _ => None,
        })
        .await;
    match param {
        Some(results) => {
            let found = results
                .get(valid_hash)
                .expect("Hash not found in results")
                .is_some();
            assert!(
                found,
                "Get blocks results does not contain the expected results."
            );
        }
        None => panic!("Get blocks results not sent before timeout."),
    }
}

pub async fn validate_block_not_found(
    protocol_controller: &mut MockProtocolController,
    valid_hash: &BlockId,
    timeout_ms: u64,
) {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::GetBlocksResults(results) => Some(results),
            _ => None,
        })
        .await;
    match param {
        Some(results) => {
            let not_found = results
                .get(valid_hash)
                .expect("Hash not found in results")
                .is_none();
            assert!(
                not_found,
                "Get blocks results does not contain the expected results."
            );
        }
        None => panic!("Get blocks results not sent before timeout."),
    }
}

pub async fn create_and_test_block(
    protocol_controller: &mut MockProtocolController,
    cfg: &ConsensusConfig,
    serialization_context: &SerializationContext,
    slot: Slot,
    best_parents: Vec<BlockId>,
    valid: bool,
    trace: bool,
) -> BlockId {
    let (block_hash, block, _) = create_block(&cfg, &serialization_context, slot, best_parents);
    if trace {
        info!("create block:{}", block_hash);
    }

    protocol_controller.receive_block(block).await;
    if valid {
        // Assert that the block is propagated.
        validate_propagate_block(protocol_controller, block_hash, 2000).await;
    } else {
        // Assert that the the block is not propagated.
        validate_notpropagate_block(protocol_controller, block_hash, 500).await;
    }
    block_hash
}

pub async fn propagate_block(
    serialization_context: &SerializationContext,
    protocol_controller: &mut MockProtocolController,
    block: Block,
    valid: bool,
) -> BlockId {
    let block_hash = block
        .header
        .compute_block_id(&serialization_context)
        .unwrap();
    protocol_controller.receive_block(block).await;
    if valid {
        //see if the block is propagated.
        validate_propagate_block(protocol_controller, block_hash, 1000).await;
    } else {
        //see if the block is propagated.
        validate_notpropagate_block(protocol_controller, block_hash, 1000).await;
    }
    block_hash
}

// returns hash and resulting discarded blocks
pub fn create_block(
    cfg: &ConsensusConfig,
    serialization_context: &SerializationContext,
    slot: Slot,
    best_parents: Vec<BlockId>,
) -> (BlockId, Block, PrivateKey) {
    create_block_with_merkle_root(
        cfg,
        serialization_context,
        Hash::hash("default_val".as_bytes()),
        slot,
        best_parents,
    )
}

// returns hash and resulting discarded blocks
pub fn create_block_with_merkle_root(
    cfg: &ConsensusConfig,
    serialization_context: &SerializationContext,
    operation_merkle_root: Hash,
    slot: Slot,
    best_parents: Vec<BlockId>,
) -> (BlockId, Block, PrivateKey) {
    let (public_key, private_key) = cfg
        .nodes
        .get(0).map(|(public_key, private_key)| (*public_key, *private_key))
        .unwrap();

    let example_hash = Hash::hash("default_val".as_bytes());

    let (hash, header) = BlockHeader::new_signed(
        &private_key,
        BlockHeaderContent {
            creator: public_key,
            slot,
            parents: best_parents,
            out_ledger_hash: example_hash,
            operation_merkle_root,
        },
        &serialization_context,
    )
    .unwrap();

    let block = Block {
        header,
        operations: Vec::new(),
    };

    (hash, block, private_key)
}

/// generate a named temporary JSON ledger file
pub fn generate_ledger_file(ledger_vec: &HashMap<Address, LedgerData>) -> NamedTempFile {
    use std::io::prelude::*;
    let ledger_file_named = NamedTempFile::new().expect("cannot create temp file");
    serde_json::to_writer_pretty(ledger_file_named.as_file(), &ledger_vec)
        .expect("unable to write ledger file");
    ledger_file_named
        .as_file()
        .seek(std::io::SeekFrom::Start(0))
        .expect("could not seek file");
    ledger_file_named
}

pub fn default_consensus_config(
    nb_nodes: usize,
    initial_ledger_path: &Path,
) -> (ConsensusConfig, SerializationContext) {
    let genesis_key = crypto::generate_random_private_key();
    let thread_count: u8 = 2;
    let max_block_size: u32 = 3 * 1024 * 1024;
    let max_operations_per_block: u32 = 1024;
    let nodes = (0..nb_nodes)
        .map(|_| {
            let private_key = crypto::generate_random_private_key();
            let public_key = crypto::derive_public_key(&private_key);
            (public_key, private_key)
        })
        .collect();
    let tempdir = tempfile::tempdir().expect("cannot create temp dir");
    (
        ConsensusConfig {
            genesis_timestamp: UTime::now(0).unwrap(),
            thread_count,
            t0: 32000.into(),
            selection_rng_seed: 42,
            genesis_key,
            nodes,
            current_node_index: 0,
            max_discarded_blocks: 10,
            future_block_processing_max_periods: 3,
            max_future_processing_blocks: 10,
            max_dependency_blocks: 10,
            delta_f0: 32,
            disable_block_creation: true,
            max_block_size,
            max_operations_per_block,
            operation_validity_periods: 3,
            ledger_path: tempdir.path().to_path_buf(),
            ledger_cache_capacity: 1000000,
            ledger_flush_interval: Some(200.into()),
            ledger_reset_at_startup: true,
            block_reward: 1,
            initial_ledger_path: initial_ledger_path.to_path_buf(),
        },
        SerializationContext {
            max_block_size,
            max_block_operations: max_operations_per_block,
            parent_count: thread_count,
            max_peer_list_length: 128,
            max_message_size: 3 * 1024 * 1024,
            max_bootstrap_blocks: 100,
            max_bootstrap_cliques: 100,
            max_bootstrap_deps: 100,
            max_bootstrap_children: 100,
            max_ask_blocks_per_message: 10,
            max_operations_per_message: 1024,
            max_bootstrap_message_size: 100000000,
        },
    )
}

pub fn get_cliques(graph: &BlockGraphExport, hash: BlockId) -> HashSet<usize> {
    let mut res = HashSet::new();
    for (i, clique) in graph.max_cliques.iter().enumerate() {
        if clique.contains(&hash) {
            res.insert(i);
        }
    }
    res
}
