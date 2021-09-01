// Copyright (c) 2021 MASSA LABS <info@massa.net>
use crate::{get_filter, ApiConfig, ApiEvent};
use communication::{network::NetworkConfig, protocol::ProtocolConfig};
use consensus::{
    get_block_slot_timestamp, BlockGraphExport, ConsensusConfig, ExportCompiledBlock,
    ExportDiscardedBlocks,
};
use crypto::{
    hash::Hash,
    signature::{derive_public_key, generate_random_private_key, PrivateKey, PublicKey},
};
use models::{Amount, Block, BlockHeader, BlockHeaderContent, BlockId, Slot, Version};
use num::rational::Ratio;
use pool::PoolConfig;
use std::str::FromStr;
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    vec,
};
use storage::{start_storage, StorageAccess, StorageConfig};
use tempfile::NamedTempFile;
use time::UTime;
use tokio::{
    sync::mpsc::{self, Receiver},
    task::JoinHandle,
};
use warp::{filters::BoxedFilter, reply::Reply};

pub fn get_dummy_block_id(s: &str) -> BlockId {
    BlockId(Hash::hash(s.as_bytes()))
}

pub fn get_test_block_id() -> BlockId {
    get_dummy_block_id("test")
}

pub fn get_another_test_block_id() -> BlockId {
    get_dummy_block_id("another test")
}

pub fn generate_staking_keys_file(staking_keys: &Vec<PrivateKey>) -> NamedTempFile {
    use std::io::prelude::*;
    let file_named = NamedTempFile::new().expect("cannot create temp file");
    serde_json::to_writer_pretty(file_named.as_file(), &staking_keys)
        .expect("unable to write ledger file");
    file_named
        .as_file()
        .seek(std::io::SeekFrom::Start(0))
        .expect("could not seek file");
    file_named
}

pub fn get_consensus_config() -> ConsensusConfig {
    let tempdir = tempfile::tempdir().expect("cannot create temp dir");
    let tempdir2 = tempfile::tempdir().expect("cannot create temp dir");
    let tempdir3 = tempfile::tempdir().expect("cannot create temp dir");
    let mut staking_keys = Vec::new();
    for _ in 0..2 {
        staking_keys.push(crypto::generate_random_private_key());
    }
    let staking_file = generate_staking_keys_file(&staking_keys);
    ConsensusConfig {
        genesis_timestamp: 0.into(),
        thread_count: 2,
        t0: 2000.into(),
        genesis_key: PrivateKey::from_bs58_check(
            "SGoTK5TJ9ZcCgQVmdfma88UdhS6GK94aFEYAsU3F1inFayQ6S",
        )
        .unwrap(),
        max_discarded_blocks: 0,
        future_block_processing_max_periods: 0,
        max_future_processing_blocks: 0,
        max_dependency_blocks: 0,
        delta_f0: 0,
        disable_block_creation: true,
        max_block_size: 1024 * 1024,
        max_operations_per_block: 1024,
        max_operations_fill_attempts: 6,
        operation_validity_periods: 3,
        ledger_path: tempdir.path().to_path_buf(),
        ledger_cache_capacity: 1000000,
        ledger_flush_interval: Some(200.into()),
        ledger_reset_at_startup: true,
        block_reward: Amount::from_str("10").unwrap(),
        initial_ledger_path: tempdir2.path().to_path_buf(),
        operation_batch_size: 100,
        initial_rolls_path: tempdir3.path().to_path_buf(),
        initial_draw_seed: "genesis".into(),
        periods_per_cycle: 100,
        pos_lookback_cycles: 2,
        pos_lock_cycles: 1,
        pos_draw_cached_cycles: 0,
        pos_miss_rate_deactivation_threshold: Ratio::new(1, 1),
        roll_price: Amount::default(),
        stats_timespan: 60000.into(),
        staking_keys_path: staking_file.path().to_path_buf(),
        end_timestamp: None,
        max_send_wait: 500.into(),
        endorsement_count: 8,
    }
}

pub fn get_protocol_config() -> ProtocolConfig {
    ProtocolConfig {
        ask_block_timeout: 10.into(),
        max_node_known_blocks_size: 100,
        max_node_wanted_blocks_size: 100,
        max_simultaneous_ask_blocks_per_node: 10,
        max_send_wait: UTime::from(100),
        max_known_ops_size: 1000,
        max_known_endorsements_size: 1000,
    }
}
pub fn get_pool_config() -> PoolConfig {
    PoolConfig {
        max_pool_size_per_thread: 100000,
        max_operation_future_validity_start_periods: 200,
        max_endorsement_count: 1000,
    }
}

pub fn initialize_context() -> models::SerializationContext {
    // Init the serialization context with a default,
    // can be overwritten with a more specific one in the test.
    let ctx = models::SerializationContext {
        max_block_operations: 1024,
        parent_count: 2,
        max_peer_list_length: 128,
        max_message_size: 3 * 1024 * 1024,
        max_block_size: 3 * 1024 * 1024,
        max_bootstrap_blocks: 100,
        max_bootstrap_cliques: 100,
        max_bootstrap_deps: 100,
        max_bootstrap_children: 100,
        max_ask_blocks_per_message: 10,
        max_operations_per_message: 1024,
        max_endorsements_per_message: 1024,
        max_bootstrap_message_size: 100000000,
        max_bootstrap_pos_entries: 1000,
        max_bootstrap_pos_cycles: 5,
        max_block_endorsments: 8,
    };
    models::init_serialization_context(ctx.clone());
    ctx
}

pub fn get_network_config() -> NetworkConfig {
    NetworkConfig {
        bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080),
        routable_ip: Some(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))),
        protocol_port: 0,
        connect_timeout: UTime::from(180_000),
        wakeup_interval: UTime::from(10_000),
        peers_file: std::path::PathBuf::new(),
        target_bootstrap_connections: 0,
        max_out_bootstrap_connection_attempts: 1,
        target_out_nonbootstrap_connections: 10,
        max_in_nonbootstrap_connections: 5,
        max_in_connections_per_ip: 2,
        max_out_nonbootstrap_connection_attempts: 15,
        max_idle_peers: 3,
        max_banned_peers: 3,
        max_advertise_length: 5,
        peers_file_dump_interval: UTime::from(10_000),
        max_message_size: 3 * 1024 * 1024,
        message_timeout: UTime::from(5000u64),
        ask_peer_list_interval: UTime::from(50000u64),
        private_key_file: std::path::PathBuf::new(),
        max_ask_blocks_per_message: 10,
        max_operations_per_message: 1024,
        max_endorsements_per_message: 1024,
        max_send_wait: UTime::from(100),
        ban_timeout: UTime::from(100_000_000),
    }
}

pub fn get_api_config() -> ApiConfig {
    ApiConfig {
        max_return_invalid_blocks: 5,
        selection_return_periods: 2,
        bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 3030),
    }
}

pub fn get_header(slot: Slot, creator: Option<PublicKey>) -> (BlockId, BlockHeader) {
    let private_key = generate_random_private_key();
    let public_key = derive_public_key(&private_key);

    BlockHeader::new_signed(
        &private_key,
        BlockHeaderContent {
            creator: creator.unwrap_or_else(|| public_key),
            slot,
            parents: Vec::new(),
            operation_merkle_root: Hash::hash(&Vec::new()),
            endorsements: Vec::new(),
        },
    )
    .unwrap()
}

pub fn mock_filter(
    storage_cmd: Option<StorageAccess>,
) -> (BoxedFilter<(impl Reply,)>, Receiver<ApiEvent>) {
    let (evt_tx, evt_rx) = mpsc::channel(1);
    (
        get_filter(
            Version::from_str("DEVE.0.0").unwrap(),
            get_api_config(),
            get_consensus_config(),
            get_protocol_config(),
            get_network_config(),
            get_pool_config(),
            evt_tx,
            storage_cmd,
            0,
        ),
        evt_rx,
    )
}

pub fn get_test_block_graph() -> BlockGraphExport {
    BlockGraphExport {
        genesis_blocks: vec![get_test_block_id(), get_another_test_block_id()],
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
    let private_key = generate_random_private_key();
    derive_public_key(&private_key)
}

pub async fn get_test_storage(cfg: ConsensusConfig) -> (StorageAccess, (Block, Block, Block)) {
    let tempdir = tempfile::tempdir().expect("cannot create temp dir");

    let storage_config = StorageConfig {
        /// Max number of bytes we want to store
        max_stored_blocks: 5,
        /// path to db
        path: tempdir.path().to_path_buf(), // in target to be ignored by git and different file between test.
        cache_capacity: 256,  // little to force flush cache
        flush_interval: None, // default
        reset_at_startup: true,
    };
    let (storage_command_tx, _storage_manager) = start_storage(storage_config).unwrap();

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
    blocks.insert(block_a.header.compute_block_id().unwrap(), block_a.clone());

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
    blocks.insert(block_b.header.compute_block_id().unwrap(), block_b.clone());

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
    blocks.insert(block_c.header.compute_block_id().unwrap(), block_c.clone());

    storage_command_tx.add_block_batch(blocks).await.unwrap();

    (storage_command_tx, (block_a, block_b, block_c))
}

pub fn get_test_block() -> Block {
    Block {
        header: BlockHeader {
            content: BlockHeaderContent {
                creator: crypto::signature::PublicKey::from_bs58_check("4vYrPNzUM8PKg2rYPW3ZnXPzy67j9fn5WsGCbnwAnk2Lf7jNHb").unwrap(),
                operation_merkle_root: Hash::hash(&Vec::new()),
                parents: vec![
                    get_dummy_block_id("parent1"),
                    get_dummy_block_id("parent2"),
                ],
                slot: Slot::new(1, 0),
                endorsements: Vec::new(),
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
    slot: Slot,
    creator: Option<PublicKey>,
) -> ExportCompiledBlock {
    ExportCompiledBlock {
        block: get_header(slot, creator).1,
        children: Vec::new(),
        status: consensus::Status::Active,
    }
}
