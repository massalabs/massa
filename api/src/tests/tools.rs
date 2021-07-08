use crate::{get_filter, ApiConfig, ApiEvent};
use communication::{network::NetworkConfig, protocol::ProtocolConfig};
use consensus::{
    get_block_slot_timestamp, BlockGraphExport, ConsensusConfig, ExportCompiledBlock,
    ExportDiscardedBlocks,
};
use crypto::{
    hash::Hash,
    signature::{PrivateKey, PublicKey, SignatureEngine},
};
use models::{Block, BlockHeader, BlockHeaderContent, SerializationContext, Slot};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    vec,
};
use storage::{start_storage, StorageAccess, StorageConfig};
use time::UTime;
use tokio::{
    sync::mpsc::{self, Receiver},
    task::JoinHandle,
};
use warp::{filters::BoxedFilter, reply::Reply};

pub fn get_test_hash() -> Hash {
    Hash::hash("test".as_bytes())
}

pub fn get_another_test_hash() -> Hash {
    Hash::hash("another test".as_bytes())
}

pub fn get_consensus_config() -> ConsensusConfig {
    ConsensusConfig {
        genesis_timestamp: 0.into(),
        thread_count: 2,
        t0: 2000.into(),
        selection_rng_seed: 0,
        genesis_key: PrivateKey::from_bs58_check(
            "SGoTK5TJ9ZcCgQVmdfma88UdhS6GK94aFEYAsU3F1inFayQ6S",
        )
        .unwrap(),
        nodes: Vec::new(),
        current_node_index: 0,
        max_discarded_blocks: 0,
        future_block_processing_max_periods: 0,
        max_future_processing_blocks: 0,
        max_dependency_blocks: 0,
        delta_f0: 0,
        disable_block_creation: true,
        max_block_size: 1024 * 1024,
        max_operations_per_block: 1024,
    }
}

pub fn get_protocol_config() -> ProtocolConfig {
    ProtocolConfig {
        ask_block_timeout: 10.into(),
        max_node_known_blocks_size: 100,
        max_node_wanted_blocks_size: 100,
        max_simultaneous_ask_blocks_per_node: 10,
    }
}

pub fn get_network_config() -> NetworkConfig {
    NetworkConfig {
        bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080),
        routable_ip: Some(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))),
        protocol_port: 0,
        connect_timeout: UTime::from(180_000),
        wakeup_interval: UTime::from(10_000),
        peers_file: std::path::PathBuf::new(),
        target_out_connections: 10,
        max_in_connections: 5,
        max_in_connections_per_ip: 2,
        max_out_connnection_attempts: 15,
        max_idle_peers: 3,
        max_banned_peers: 3,
        max_advertise_length: 5,
        peers_file_dump_interval: UTime::from(10_000),
        max_message_size: 3 * 1024 * 1024,
        message_timeout: UTime::from(5000u64),
        ask_peer_list_interval: UTime::from(50000u64),
        private_key_file: std::path::PathBuf::new(),
        max_ask_blocks_per_message: 10,
    }
}

pub fn get_api_config() -> ApiConfig {
    ApiConfig {
        max_return_invalid_blocks: 5,
        selection_return_periods: 2,
        bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 3030),
    }
}

pub fn get_header(
    serialization_context: &SerializationContext,
    slot: Slot,
    creator: Option<PublicKey>,
) -> (Hash, BlockHeader) {
    let mut sig_engine = SignatureEngine::new();
    let private_key = SignatureEngine::generate_random_private_key();
    let public_key = sig_engine.derive_public_key(&private_key);

    BlockHeader::new_signed(
        &mut sig_engine,
        &private_key,
        BlockHeaderContent {
            creator: if creator.is_none() {
                public_key
            } else {
                creator.unwrap()
            },
            slot,
            parents: Vec::new(),
            out_ledger_hash: get_test_hash(),
            operation_merkle_root: get_another_test_hash(),
        },
        serialization_context,
    )
    .unwrap()
}

pub fn mock_filter(
    storage_cmd: Option<StorageAccess>,
) -> (BoxedFilter<(impl Reply,)>, Receiver<ApiEvent>) {
    let (evt_tx, evt_rx) = mpsc::channel(1);
    (
        get_filter(
            get_api_config(),
            get_consensus_config(),
            get_protocol_config(),
            get_network_config(),
            evt_tx,
            storage_cmd,
            0,
        ),
        evt_rx,
    )
}

pub fn get_test_block_graph() -> BlockGraphExport {
    BlockGraphExport {
        genesis_blocks: vec![get_test_hash(), get_another_test_hash()],
        active_blocks: HashMap::new(),
        discarded_blocks: ExportDiscardedBlocks {
            map: HashMap::new(),
        },
        best_parents: Vec::new(),
        latest_final_blocks_periods: Vec::new(),
        gi_head: HashMap::new(),
        max_cliques: Vec::new(),
    }
}

pub fn get_dummy_staker() -> PublicKey {
    let signature_engine = SignatureEngine::new();
    let private_key = SignatureEngine::generate_random_private_key();
    signature_engine.derive_public_key(&private_key)
}

pub async fn get_test_storage(
    cfg: ConsensusConfig,
    serialization_context: SerializationContext,
) -> (StorageAccess, (Block, Block, Block)) {
    let tempdir = tempfile::tempdir().expect("cannot create temp dir");

    let storage_config = StorageConfig {
        /// Max number of bytes we want to store
        max_stored_blocks: 5,
        /// path to db
        path: tempdir.path().to_path_buf(), //in target to be ignored by git and different file between test.
        cache_capacity: 256,  //little to force flush cache
        flush_interval: None, //defaut
        reset_at_startup: true,
    };
    let (storage_command_tx, _storage_manager) =
        start_storage(storage_config, serialization_context.clone()).unwrap();

    let mut blocks = HashMap::new();

    let mut block_a = get_test_block();
    block_a.header.content.slot = Slot::new(1, 0);
    assert_eq!(
        get_block_slot_timestamp(
            cfg.thread_count,
            cfg.t0,
            cfg.genesis_timestamp,
            block_a.header.content.slot
        )
        .unwrap(),
        2000.into()
    );
    blocks.insert(
        block_a
            .header
            .content
            .compute_hash(&serialization_context)
            .unwrap(),
        block_a.clone(),
    );

    let mut block_b = get_test_block();
    block_b.header.content.slot = Slot::new(1, 1);
    assert_eq!(
        get_block_slot_timestamp(
            cfg.thread_count,
            cfg.t0,
            cfg.genesis_timestamp,
            block_b.header.content.slot
        )
        .unwrap(),
        3000.into()
    );
    blocks.insert(
        block_b
            .header
            .content
            .compute_hash(&serialization_context)
            .unwrap(),
        block_b.clone(),
    );

    let mut block_c = get_test_block();
    block_c.header.content.slot = Slot::new(2, 0);
    assert_eq!(
        get_block_slot_timestamp(
            cfg.thread_count,
            cfg.t0,
            cfg.genesis_timestamp,
            block_c.header.content.slot
        )
        .unwrap(),
        4000.into()
    );
    blocks.insert(
        block_c
            .header
            .content
            .compute_hash(&serialization_context)
            .unwrap(),
        block_c.clone(),
    );

    storage_command_tx.add_block_batch(blocks).await.unwrap();

    (storage_command_tx, (block_a, block_b, block_c))
}

pub fn get_test_block() -> Block {
    Block {
        header: BlockHeader {
            content: BlockHeaderContent {
                creator: crypto::signature::PublicKey::from_bs58_check("4vYrPNzUM8PKg2rYPW3ZnXPzy67j9fn5WsGCbnwAnk2Lf7jNHb").unwrap(),
                operation_merkle_root: get_test_hash(),
                out_ledger_hash: get_test_hash(),
                parents: vec![
                    Hash::hash("parent1".as_bytes()),
                    Hash::hash("parent2".as_bytes())
                ],
                slot: Slot::new(1, 0),
            },
            signature: crypto::signature::Signature::from_bs58_check(
                "5f4E3opXPWc3A1gvRVV7DJufvabDfaLkT1GMterpJXqRZ5B7bxPe5LoNzGDQp9LkphQuChBN1R5yEvVJqanbjx7mgLEae"
            ).unwrap()
        },
        operations: vec![],
    }
}

pub fn get_empty_graph_handle(mut rx_api: Receiver<ApiEvent>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let evt = rx_api.recv().await;
        match evt {
            Some(ApiEvent::GetBlockGraphStatus(response_sender_tx)) => {
                response_sender_tx
                    .send(get_test_block_graph())
                    .expect("failed to send block graph");
            }
            None => {}
            _ => {}
        }
    })
}
pub fn get_test_compiled_exported_block(
    serialization_context: &SerializationContext,
    slot: Slot,
    creator: Option<PublicKey>,
) -> ExportCompiledBlock {
    ExportCompiledBlock {
        block: get_header(&serialization_context, slot, creator).1,
        children: Vec::new(),
        status: consensus::Status::Active,
    }
}
