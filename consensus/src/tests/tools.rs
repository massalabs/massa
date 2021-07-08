use super::mock_protocol_controller::MockProtocolController;
use crate::{block_graph::BlockGraphExport, ConsensusConfig};
use communication::protocol::ProtocolCommand;
use crypto::{
    hash::Hash,
    signature::{PrivateKey, SignatureEngine},
};
use models::{Block, BlockHeader, BlockHeaderContent, SerializationContext, Slot};
use std::collections::HashSet;
use storage::{StorageAccess, StorageConfig};
use time::UTime;

//return true if another block has been seen
pub async fn validate_notpropagate_block(
    protocol_controller: &mut MockProtocolController,
    not_propagated_hash: Hash,
    timeout_ms: u64,
) -> bool {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::PropagateBlockHeader { hash, .. } => return Some(hash),
            _ => None,
        })
        .await;
    match param {
        Some(hash) => !(not_propagated_hash == hash),
        None => false,
    }
}

//return true if another block has been seen
pub async fn validate_notpropagate_block_in_list(
    protocol_controller: &mut MockProtocolController,
    not_propagated_hashs: &Vec<Hash>,
    timeout_ms: u64,
) -> bool {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::PropagateBlockHeader { hash, .. } => return Some(hash),
            _ => None,
        })
        .await;
    match param {
        Some(hash) => !not_propagated_hashs.contains(&hash),
        None => false,
    }
}

pub async fn validate_propagate_block_in_list(
    protocol_controller: &mut MockProtocolController,
    valid_hashs: &Vec<Hash>,
    timeout_ms: u64,
) -> Hash {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::PropagateBlockHeader { hash, .. } => return Some(hash),
            _ => None,
        })
        .await;
    match param {
        Some(hash) => {
            trace!("receive hash:{}", hash);
            assert!(valid_hashs.contains(&hash), "not the valid hash propagated");
            hash
        }
        None => panic!("Hash not propagated."),
    }
}

pub async fn validate_ask_for_block(
    protocol_controller: &mut MockProtocolController,
    valid_hash: Hash,
    timeout_ms: u64,
) -> Hash {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::WishlistDelta { new, .. } => return Some(new),
            _ => None,
        })
        .await;
    match param {
        Some(new) => {
            trace!("asking for hashes:{:?}", new);
            assert!(new.contains(&valid_hash), "not the valid hash asked for");
            assert_eq!(new.len(), 1);
            valid_hash
        }
        None => panic!("Block not asked for before timeout."),
    }
}

pub async fn validate_does_not_ask_for_block(
    protocol_controller: &mut MockProtocolController,
    hash: &Hash,
    timeout_ms: u64,
) {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::WishlistDelta { new, .. } => return Some(new),
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
    valid_hash: Hash,
    timeout_ms: u64,
) {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::PropagateBlockHeader { hash, .. } => return Some(hash),
            _ => None,
        })
        .await;
    match param {
        Some(hash) => assert_eq!(valid_hash, hash, "not the valid hash propagated"),
        None => panic!("Block not propagated before timeout."),
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
    };
    let (storage_command_tx, _storage_manager) =
        storage::start_storage(storage_config, serialization_context.clone()).unwrap();
    storage_command_tx
}

pub async fn validate_send_block(
    protocol_controller: &mut MockProtocolController,
    valid_hash: Hash,
    timeout_ms: u64,
) {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::SendBlock { hash, .. } => return Some(hash),
            _ => None,
        })
        .await;
    match param {
        Some(hash) => assert_eq!(valid_hash, hash, "not the valid hash propagated"),
        None => panic!("Block not sent before timeout."),
    }
}

pub async fn create_and_test_block(
    protocol_controller: &mut MockProtocolController,
    cfg: &ConsensusConfig,
    serialization_context: &SerializationContext,
    slot: Slot,
    best_parents: Vec<Hash>,
    valid: bool,
    trace: bool,
) -> Hash {
    let (block_hash, block, _) = create_block(&cfg, &serialization_context, slot, best_parents);
    if trace {
        info!("create block:{}", block_hash);
    }

    protocol_controller.receive_block(block).await;
    if valid {
        // Assert that the block is propagated.
        validate_propagate_block(protocol_controller, block_hash, 1000).await;
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
) -> Hash {
    let block_hash = block
        .header
        .content
        .compute_hash(&serialization_context)
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
    best_parents: Vec<Hash>,
) -> (Hash, Block, PrivateKey) {
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
    best_parents: Vec<Hash>,
) -> (Hash, Block, PrivateKey) {
    let mut signature_engine = SignatureEngine::new();
    let (public_key, private_key) = cfg
        .nodes
        .get(0)
        .and_then(|(public_key, private_key)| Some((public_key.clone(), private_key.clone())))
        .unwrap();

    let example_hash = Hash::hash("default_val".as_bytes());

    let (hash, header) = BlockHeader::new_signed(
        &mut signature_engine,
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

pub fn default_consensus_config(nb_nodes: usize) -> (ConsensusConfig, SerializationContext) {
    let genesis_key = SignatureEngine::generate_random_private_key();
    let thread_count: u8 = 2;
    let max_block_size: u32 = 3 * 1024 * 1024;
    let max_operations_per_block: u32 = 1024;
    let signature_engine = SignatureEngine::new();
    let nodes = (0..nb_nodes)
        .map(|_| {
            let private_key = SignatureEngine::generate_random_private_key();
            let public_key = signature_engine.derive_public_key(&private_key);
            (public_key, private_key)
        })
        .collect();
    (
        ConsensusConfig {
            genesis_timestamp: UTime::now().unwrap(),
            thread_count: thread_count,
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
        },
        SerializationContext {
            max_block_size,
            max_block_operations: max_operations_per_block,
            parent_count: thread_count,
            max_peer_list_length: 128,
            max_message_size: 3 * 1024 * 1024,
        },
    )
}

pub fn get_cliques(graph: &BlockGraphExport, hash: Hash) -> HashSet<usize> {
    let mut res = HashSet::new();
    for (i, clique) in graph.max_cliques.iter().enumerate() {
        if clique.contains(&hash) {
            res.insert(i);
        }
    }
    res
}
