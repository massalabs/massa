use super::super::config::ConsensusConfig;
use super::mock_protocol_controller::{MockProtocolCommand, MockProtocolControllerInterface};
use crate::block_graph::BlockGraphExport;
use communication::protocol::protocol_controller::NodeId;
use crypto::signature::PrivateKey;
use crypto::{hash::Hash, signature::SignatureEngine};
use models::block::Block;
use models::block::BlockHeader;
use std::{collections::HashSet, time::Duration};
use time::UTime;

use tokio::time::timeout;

//return true if another block has been seen
pub async fn validate_notpropagate_block(
    protocol_controler_interface: &mut MockProtocolControllerInterface,
    not_propagated_hash: Hash,
    timeout_ms: u64,
) -> bool {
    match timeout(
        Duration::from_millis(timeout_ms),
        protocol_controler_interface.wait_command(),
    )
    .await
    {
        Ok(Some(MockProtocolCommand::PropagateBlock { hash, .. })) => {
            if not_propagated_hash == hash {
                panic!("validate_notpropagate_block the block has been propagated.")
            } else {
                true
            }
        }
        Ok(None) => panic!("an error occurs while waiting for ProtocolCommand event"),
        Err(_) => false,
    }
}

//return true if another block has been seen
pub async fn validate_notpropagate_block_in_list(
    protocol_controler_interface: &mut MockProtocolControllerInterface,
    not_propagated_hashs: &Vec<Hash>,
    timeout_ms: u64,
) -> bool {
    match timeout(
        Duration::from_millis(timeout_ms),
        protocol_controler_interface.wait_command(),
    )
    .await
    {
        Ok(Some(MockProtocolCommand::PropagateBlock { hash, .. })) => if not_propagated_hashs.contains(&hash) {
            panic!(
                "validate_notpropagate_block the block has been propagated."
            )
        } else {
            true
        }
        ,
        //        event @ Ok(Some(_)) => panic!("unexpected event sent by Protocol: {:?}", event),
        Ok(None) => panic!("an error occurs while waiting for ProtocolCommand event"),
        Err(_) => false,
    }
}

pub async fn validate_propagate_block_in_list(
    protocol_controler_interface: &mut MockProtocolControllerInterface,
    valid_hashs: &Vec<Hash>,
    timeout_ms: u64,
) -> Hash {
    match timeout(
        Duration::from_millis(timeout_ms),
        protocol_controler_interface.wait_command(),
    )
    .await
    {
        Ok(Some(MockProtocolCommand::PropagateBlock { hash, .. })) => {
            trace!("receive hash:{}", hash);
            assert!(valid_hashs.contains(&hash), "not the valid hash propagated");
            hash
        }
        //        event @ Ok(Some(_)) => panic!("unexpected event sent by Protocol: {:?}", event),
        Ok(None) => panic!("an error occurs while waiting for ProtocolCommand event"),
        Err(_) => panic!("timeout block not propagated"),
    }
}

pub async fn validate_propagate_block(
    protocol_controler_interface: &mut MockProtocolControllerInterface,
    valid_hash: Hash,
    timeout_ms: u64,
) {
    match timeout(
        Duration::from_millis(timeout_ms),
        protocol_controler_interface.wait_command(),
    )
    .await
    {
        Ok(Some(MockProtocolCommand::PropagateBlock { hash, .. })) => {
            //skip created block
            if valid_hash != hash {
                match timeout(
                    Duration::from_millis(timeout_ms),
                    protocol_controler_interface.wait_command(),
                )
                .await
                {
                    Ok(Some(MockProtocolCommand::PropagateBlock { hash, .. })) => {
                        assert_eq!(valid_hash, hash, "not the valid hash propagated")
                    }
                    Ok(None) => panic!("an error occurs while waiting for ProtocolCommand event"),
                    Err(_) => panic!("timeout block not propagated"),
                }
            }
            //assert_eq!(valid_hash, hash, "not the valid hash propagated")
        }
        //        event @ Ok(Some(_)) => panic!("unexpected event sent by Protocol: {:?}", event),
        Ok(None) => panic!("an error occurs while waiting for ProtocolCommand event"),
        Err(_) => panic!("timeout block not propagated"),
    };
}

pub fn create_node_ids(nb_nodes: usize) -> Vec<(PrivateKey, NodeId)> {
    let signature_engine = SignatureEngine::new();
    (0..nb_nodes)
        .map(|_| {
            let private_key = SignatureEngine::generate_random_private_key();
            let self_node_id = NodeId(signature_engine.derive_public_key(&private_key));
            (private_key, self_node_id)
        })
        .collect()
}

pub async fn create_and_test_block(
    protocol_controler_interface: &mut MockProtocolControllerInterface,
    cfg: &ConsensusConfig,
    source_node_id: NodeId,
    thread_number: u8,
    period_number: u64,
    best_parents: Vec<Hash>,
    valid: bool,
    trace: bool,
) -> Hash {
    let (block_hash, block, _) = create_block(&cfg, thread_number, period_number, best_parents);
    if trace {
        info!("create block:{}", block_hash);
    }

    protocol_controler_interface
        .receive_block(source_node_id, &block)
        .await;
    if valid {
        //see if the block is propagated.
        validate_propagate_block(protocol_controler_interface, block_hash, 1000).await;
    } else {
        //see if the block is propagated.
        validate_notpropagate_block(protocol_controler_interface, block_hash, 500).await;
    }
    block_hash
}

pub async fn propagate_block(
    protocol_controler_interface: &mut MockProtocolControllerInterface,
    source_node_id: NodeId,
    block: Block,
    valid: bool,
) -> Hash {
    let block_hash = block.header.compute_hash().unwrap();
    protocol_controler_interface
        .receive_block(source_node_id, &block)
        .await;
    if valid {
        //see if the block is propagated.
        validate_propagate_block(protocol_controler_interface, block_hash, 1000).await;
    } else {
        //see if the block is propagated.
        validate_notpropagate_block(protocol_controler_interface, block_hash, 1000).await;
    }
    block_hash
}

// returns hash and resulting discarded blocks
pub fn create_block(
    cfg: &ConsensusConfig,
    thread_number: u8,
    period_number: u64,
    best_parents: Vec<Hash>,
) -> (Hash, Block, PrivateKey) {
    create_block_with_merkle_root(
        cfg,
        Hash::hash("default_val".as_bytes()),
        thread_number,
        period_number,
        best_parents,
    )
}
// returns hash and resulting discarded blocks
pub fn create_block_with_merkle_root(
    cfg: &ConsensusConfig,
    operation_merkle_root: Hash,
    thread_number: u8,
    period_number: u64,
    best_parents: Vec<Hash>,
) -> (Hash, Block, PrivateKey) {
    let signature_engine = SignatureEngine::new();
    let (public_key, private_key) = cfg
        .nodes
        .get(0)
        .and_then(|(public_key, private_key)| Some((public_key.clone(), private_key.clone())))
        .unwrap();

    let example_hash = Hash::hash("default_val".as_bytes());

    let header = BlockHeader {
        creator: public_key,
        thread_number,
        period_number,
        roll_number: cfg.current_node_index,
        parents: best_parents,
        endorsements: Vec::new(),
        out_ledger_hash: example_hash,
        operation_merkle_root,
    };

    let hash = header.compute_hash().unwrap();

    let block = Block {
        header,
        operations: Vec::new(),
        signature: signature_engine.sign(&hash, &private_key).unwrap(),
    };

    (hash, block, private_key)
}

pub fn default_consensus_config(nodes: &[(PrivateKey, NodeId)]) -> ConsensusConfig {
    let genesis_key = SignatureEngine::generate_random_private_key();

    ConsensusConfig {
        genesis_timestamp: UTime::now().unwrap(),
        thread_count: 2,
        t0: 32000.into(),
        selection_rng_seed: 42,
        genesis_key,
        nodes: nodes
            .iter()
            .map(|(pk, nodeid)| (nodeid.0, pk.clone()))
            .collect(),
        current_node_index: 0,
        max_discarded_blocks: 10,
        future_block_processing_max_periods: 3,
        max_future_processing_blocks: 10,
        max_dependency_blocks: 10,
        delta_f0: 32,
        disable_block_creation: true,
    }
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
