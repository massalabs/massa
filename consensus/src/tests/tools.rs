use super::mock_protocol_controller::MockProtocolController;
use crate::{block_graph::BlockGraphExport, ConsensusConfig};
use communication::protocol::ProtocolCommand;
use communication::NodeId;
use crypto::{
    hash::Hash,
    signature::{PrivateKey, PublicKey, SignatureEngine},
};
use models::{Block, BlockHeader, BlockHeaderContent, SerializationContext, Slot};
use std::{collections::HashSet, time::Duration};
use time::UTime;

use tokio::time::timeout;

//return true if another block has been seen
pub async fn validate_notpropagate_block(
    protocol_controller: &mut MockProtocolController,
    not_propagated_hash: Hash,
    timeout_ms: u64,
) -> bool {
    loop {
        match timeout(
            Duration::from_millis(timeout_ms),
            protocol_controller.wait_command(),
        )
        .await
        {
            Ok(Some(ProtocolCommand::PropagateBlockHeader { hash, .. })) => {
                if not_propagated_hash == hash {
                    panic!("validate_notpropagate_block the block has been propagated.")
                } else {
                    return true;
                }
            }
            Ok(Some(cmd)) => {}
            Ok(None) => panic!("an error occurs while waiting for ProtocolCommand event"),
            Err(_) => return false,
        }
    }
}

//return true if another block has been seen
pub async fn validate_notpropagate_block_in_list(
    protocol_controller: &mut MockProtocolController,
    not_propagated_hashs: &Vec<Hash>,
    timeout_ms: u64,
) -> bool {
    loop {
        match timeout(
            Duration::from_millis(timeout_ms),
            protocol_controller.wait_command(),
        )
        .await
        {
            Ok(Some(ProtocolCommand::PropagateBlockHeader { hash, .. })) => {
                if not_propagated_hashs.contains(&hash) {
                    panic!("validate_notpropagate_block the block has been propagated.")
                } else {
                    return true;
                }
            }
            Ok(Some(cmd)) => {}
            Ok(None) => panic!("an error occurs while waiting for ProtocolCommand event"),
            Err(_) => return false,
        }
    }
}

pub async fn validate_propagate_block_in_list(
    protocol_controller: &mut MockProtocolController,
    valid_hashs: &Vec<Hash>,
    timeout_ms: u64,
) -> Hash {
    loop {
        match timeout(
            Duration::from_millis(timeout_ms),
            protocol_controller.wait_command(),
        )
        .await
        {
            Ok(Some(ProtocolCommand::PropagateBlockHeader { hash, .. })) => {
                trace!("receive hash:{}", hash);
                assert!(valid_hashs.contains(&hash), "not the valid hash propagated");
                return hash;
            }
            Ok(Some(cmd)) => {}
            Ok(msg) => panic!(
                "an error occurs while waiting for ProtocolCommand event {:?}",
                msg
            ),
            Err(_) => panic!("timeout block not propagated {:?}", valid_hashs),
        }
    }
}

pub async fn validate_asks_for_block_in_list(
    protocol_controller: &mut MockProtocolController,
    valid_hashs: &Vec<Hash>,
    timeout_ms: u64,
) -> Hash {
    match timeout(
        Duration::from_millis(timeout_ms),
        protocol_controller.wait_command(),
    )
    .await
    {
        Ok(Some(ProtocolCommand::AskForBlock(hash))) => {
            trace!("receive hash:{}", hash);
            assert!(valid_hashs.contains(&hash), "not the valid hash asked for");
            hash
        }
        Ok(Some(cmd)) => panic!("unexpected command {:?}", cmd),
        Ok(msg) => panic!(
            "an error occurs while waiting for ProtocolCommand event {:?}",
            msg
        ),
        Err(_) => panic!("timeout block not asked for"),
    }
}

pub async fn validate_does_not_ask_for_block_in_list(
    protocol_controller: &mut MockProtocolController,
    _valid_hashs: &Vec<Hash>,
    timeout_ms: u64,
) {
    match timeout(
        Duration::from_millis(timeout_ms),
        protocol_controller.wait_command(),
    )
    .await
    {
        Ok(Some(ProtocolCommand::AskForBlock(hash))) => {
            panic!("unexpected ask for block {:?}", hash);
        }
        Ok(Some(cmd)) => panic!("unexpected command {:?}", cmd),
        Ok(msg) => panic!(
            "an error occurs while waiting for ProtocolCommand event {:?}",
            msg
        ),
        Err(_) => {}
    }
}

pub async fn validate_propagate_block(
    protocol_controller: &mut MockProtocolController,
    valid_hash: Hash,
    timeout_ms: u64,
) {
    match timeout(
        Duration::from_millis(timeout_ms),
        protocol_controller.wait_command(),
    )
    .await
    {
        Ok(Some(ProtocolCommand::PropagateBlockHeader { hash, .. })) => {
            //skip created block
            if valid_hash != hash {
                match timeout(
                    Duration::from_millis(timeout_ms),
                    protocol_controller.wait_command(),
                )
                .await
                {
                    Ok(Some(ProtocolCommand::PropagateBlockHeader { hash, .. })) => {
                        assert_eq!(valid_hash, hash, "not the valid hash propagated")
                    }
                    Ok(Some(cmd)) => panic!("unexpected command {:?}", cmd),
                    Ok(None) => panic!("an error occurs while waiting for ProtocolCommand event"),
                    Err(_) => panic!("timeout block not propagated"),
                }
            }
            //assert_eq!(valid_hash, hash, "not the valid hash propagated")
        }
        //        event @ Ok(Some(_)) => panic!("unexpected event sent by Protocol: {:?}", event),
        Ok(Some(cmd)) => panic!("unexpected command {:?}", cmd),
        Ok(None) => panic!("an error occurs while waiting for ProtocolCommand event"),
        Err(_) => panic!("timeout block not propagated"),
    };
}

pub async fn validate_send_block(
    protocol_controller: &mut MockProtocolController,
    valid_hash: Hash,
    timeout_ms: u64,
) {
    match timeout(
        Duration::from_millis(timeout_ms),
        protocol_controller.wait_command(),
    )
    .await
    {
        Ok(Some(ProtocolCommand::SendBlock { hash, .. })) => {
            assert_eq!(valid_hash, hash, "not the valid hash propagated");
        }
        Ok(Some(cmd)) => panic!("unexpected command {:?}", cmd),
        Ok(None) => panic!("an error occurs while waiting for ProtocolCommand event"),
        Err(_) => panic!("timeout block not propagated"),
    };
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
        //see if the block is propagated.
        validate_propagate_block(protocol_controller, block_hash, 1000).await;
    } else {
        //see if the block is propagated.
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
