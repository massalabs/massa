use num::rational::Ratio;
use std::{collections::HashMap, fs::read_to_string, time::Duration};

use massa_consensus_exports::test_exports::ConsensusControllerImpl;
use massa_metrics::MassaMetrics;
use massa_models::config::MIP_STORE_STATS_BLOCK_CONSIDERED;
use massa_pool_exports::test_exports::MockPoolController;
use massa_pos_exports::test_exports::MockSelectorController;
use massa_protocol_exports::{PeerCategoryInfo, PeerData, PeerId, ProtocolConfig};
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_versioning::versioning::{MipStatsConfig, MipStore};
use peernet::transports::TransportType;
use tempfile::NamedTempFile;

use crate::{create_protocol_controller, start_protocol_controller};

mod ban_nodes_scenarios;
mod block_scenarios;
mod cache_scenarios;
mod context;
mod endorsements_scenarios;
mod in_block_operations_scenarios;
mod mock_network;
mod operations_scenarios;
mod tools;

#[test]
fn basic() {
    // Setup panic handlers,
    // and when a panic occurs,
    // run default handler,
    // and then shutdown.
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let (pool_controller1, _) = MockPoolController::new_with_receiver();
    let (pool_controller2, _) = MockPoolController::new_with_receiver();

    let (consensus_controller1, _) = ConsensusControllerImpl::new_with_receiver();
    let (consensus_controller2, _) = ConsensusControllerImpl::new_with_receiver();

    let (selector_controller1, _) = MockSelectorController::new_with_receiver();
    let (selector_controller2, _) = MockSelectorController::new_with_receiver();
    // Setup the configs
    let mut config1 = ProtocolConfig::default();
    config1
        .listeners
        .insert("127.0.0.1:8081".parse().unwrap(), TransportType::Tcp);
    config1.keypair_file = "./src/tests/test_keypair1.json".to_string().into();
    let keypair_bs58_check_encoded = read_to_string(&config1.keypair_file)
        .map_err(|err| {
            std::io::Error::new(err.kind(), format!("could not load node key file: {}", err))
        })
        .unwrap();
    let keypair1 =
        serde_json::from_slice::<KeyPair>(keypair_bs58_check_encoded.as_bytes()).unwrap();
    let mut config2 = ProtocolConfig::default();
    config2
        .listeners
        .insert("127.0.0.1:8082".parse().unwrap(), TransportType::Tcp);
    config2.keypair_file = "./src/tests/test_keypair2.json".to_string().into();
    let keypair_bs58_check_encoded = read_to_string(&config2.keypair_file)
        .map_err(|err| {
            std::io::Error::new(err.kind(), format!("could not load node key file: {}", err))
        })
        .unwrap();
    let keypair2 =
        serde_json::from_slice::<KeyPair>(keypair_bs58_check_encoded.as_bytes()).unwrap();

    // Setup initial peers
    let initial_peers_file = NamedTempFile::new().expect("cannot create temp file");
    let mut initial_peers1: HashMap<PeerId, PeerData> = HashMap::new();
    let mut peers_1 = HashMap::new();
    peers_1.insert("127.0.0.1:8082".parse().unwrap(), TransportType::Tcp);
    initial_peers1.insert(
        PeerId::from_public_key(keypair2.get_public_key()),
        PeerData {
            listeners: peers_1,
            category: "Bootstrap".to_string(),
        },
    );
    serde_json::to_writer_pretty(initial_peers_file.as_file(), &initial_peers1)
        .expect("unable to write ledger file");
    let initial_peers_file_2 = NamedTempFile::new().expect("cannot create temp file");
    let mut initial_peers2: HashMap<PeerId, PeerData> = HashMap::new();
    let mut peers_2 = HashMap::new();
    peers_2.insert("127.0.0.1:8081".parse().unwrap(), TransportType::Tcp);
    initial_peers2.insert(
        PeerId::from_public_key(keypair1.get_public_key()),
        PeerData {
            listeners: peers_2,
            category: "Bootstrap".to_string(),
        },
    );
    serde_json::to_writer_pretty(initial_peers_file_2.as_file(), &initial_peers2)
        .expect("unable to write ledger file");
    config1.initial_peers = initial_peers_file.path().to_path_buf();
    let mut categories = HashMap::default();
    categories.insert(
        "Bootstrap".to_string(),
        PeerCategoryInfo {
            allow_local_peers: true,
            max_in_connections: 1,
            target_out_connections: 1,
            max_in_connections_per_ip: 1,
        },
    );
    config1.peers_categories = categories;
    config2.initial_peers = initial_peers_file_2.path().to_path_buf();
    let mut categories2 = HashMap::default();
    categories2.insert(
        "Bootstrap".to_string(),
        PeerCategoryInfo {
            allow_local_peers: true,
            max_in_connections: 5,
            target_out_connections: 1,
            max_in_connections_per_ip: 1,
        },
    );
    config2.peers_categories = categories2;
    config2.debug = false;

    // Setup the storages
    let storage1 = Storage::create_root();
    let storage2 = Storage::create_root();

    let (mut sender_manager1, channels1) = create_protocol_controller(config1.clone());
    let (mut sender_manager2, channels2) = create_protocol_controller(config2.clone());

    // Setup the MIP store
    let mip_stats_config = MipStatsConfig {
        block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
        warn_announced_version_ratio: Ratio::new_raw(30, 100),
    };
    let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

    let metrics = MassaMetrics::new(
        false,
        "0.0.0.0:9898".parse().unwrap(),
        32,
        std::time::Duration::from_secs(5),
    )
    .0;

    // Setup the protocols
    let (mut manager1, _, _) = start_protocol_controller(
        config1,
        selector_controller1,
        consensus_controller1,
        None,
        pool_controller1,
        storage1,
        channels1,
        mip_store.clone(),
        metrics.clone(),
    )
    .expect("Failed to start protocol 1");
    let (mut manager2, _, _) = start_protocol_controller(
        config2,
        selector_controller2,
        consensus_controller2,
        None,
        pool_controller2,
        storage2,
        channels2,
        mip_store,
        metrics,
    )
    .expect("Failed to start protocol 2");

    std::thread::sleep(Duration::from_secs(15));
    // Stop the protocols
    sender_manager1.stop();
    manager1.stop();
    sender_manager2.stop();
    manager2.stop();
}

#[test]
fn stop_with_controller_still_exists() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let (pool_controller1, _) = MockPoolController::new_with_receiver();
    let (pool_controller2, _) = MockPoolController::new_with_receiver();

    let (consensus_controller1, _) = ConsensusControllerImpl::new_with_receiver();
    let (consensus_controller2, _) = ConsensusControllerImpl::new_with_receiver();

    let (selector_controller1, _) = MockSelectorController::new_with_receiver();
    let (selector_controller2, _) = MockSelectorController::new_with_receiver();
    // Setup the configs
    let mut config1 = ProtocolConfig::default();
    config1
        .listeners
        .insert("127.0.0.1:8083".parse().unwrap(), TransportType::Tcp);
    config1.keypair_file = "./src/tests/test_keypair1.json".to_string().into();
    let keypair_bs58_check_encoded = read_to_string(&config1.keypair_file)
        .map_err(|err| {
            std::io::Error::new(err.kind(), format!("could not load node key file: {}", err))
        })
        .unwrap();
    let keypair1 =
        serde_json::from_slice::<KeyPair>(keypair_bs58_check_encoded.as_bytes()).unwrap();
    let mut config2 = ProtocolConfig::default();
    config2
        .listeners
        .insert("127.0.0.1:8086".parse().unwrap(), TransportType::Tcp);
    config2.keypair_file = "./src/tests/test_keypair2.json".to_string().into();
    let keypair_bs58_check_encoded = read_to_string(&config2.keypair_file)
        .map_err(|err| {
            std::io::Error::new(err.kind(), format!("could not load node key file: {}", err))
        })
        .unwrap();
    let keypair2 =
        serde_json::from_slice::<KeyPair>(keypair_bs58_check_encoded.as_bytes()).unwrap();

    // Setup initial peers
    let initial_peers_file = NamedTempFile::new().expect("cannot create temp file");
    let mut initial_peers1: HashMap<PeerId, PeerData> = HashMap::new();
    let mut peers_1 = HashMap::new();
    peers_1.insert("127.0.0.1:8086".parse().unwrap(), TransportType::Tcp);
    initial_peers1.insert(
        PeerId::from_public_key(keypair2.get_public_key()),
        PeerData {
            listeners: peers_1,
            category: "Bootstrap".to_string(),
        },
    );
    serde_json::to_writer_pretty(initial_peers_file.as_file(), &initial_peers1)
        .expect("unable to write ledger file");
    let initial_peers_file_2 = NamedTempFile::new().expect("cannot create temp file");
    let mut initial_peers2: HashMap<PeerId, PeerData> = HashMap::new();
    let mut peers_2 = HashMap::new();
    peers_2.insert("127.0.0.1:8083".parse().unwrap(), TransportType::Tcp);
    initial_peers2.insert(
        PeerId::from_public_key(keypair1.get_public_key()),
        PeerData {
            listeners: peers_2,
            category: "Bootstrap".to_string(),
        },
    );
    serde_json::to_writer_pretty(initial_peers_file_2.as_file(), &initial_peers2)
        .expect("unable to write ledger file");
    config1.initial_peers = initial_peers_file.path().to_path_buf();
    let mut categories = HashMap::default();
    categories.insert(
        "Bootstrap".to_string(),
        PeerCategoryInfo {
            allow_local_peers: true,
            max_in_connections: 1,
            target_out_connections: 1,
            max_in_connections_per_ip: 1,
        },
    );
    config1.peers_categories = categories;
    config2.initial_peers = initial_peers_file_2.path().to_path_buf();
    let mut categories2 = HashMap::default();
    categories2.insert(
        "Bootstrap".to_string(),
        PeerCategoryInfo {
            allow_local_peers: true,
            max_in_connections: 5,
            target_out_connections: 1,
            max_in_connections_per_ip: 1,
        },
    );
    config2.peers_categories = categories2;
    config2.debug = false;

    // Setup the storages
    let storage1 = Storage::create_root();
    let storage2 = Storage::create_root();

    // Setup the MIP store
    let mip_stats_config = MipStatsConfig {
        block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
        warn_announced_version_ratio: Ratio::new_raw(30, 100),
    };
    let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();
    let metrics = MassaMetrics::new(
        false,
        "0.0.0.0:9898".parse().unwrap(),
        32,
        std::time::Duration::from_secs(5),
    )
    .0;

    // Setup the protocols
    let (mut sender_manager1, channels1) = create_protocol_controller(config1.clone());
    let (mut sender_manager2, channels2) = create_protocol_controller(config2.clone());
    let (mut manager1, _, _) = start_protocol_controller(
        config1,
        selector_controller1,
        consensus_controller1,
        None,
        pool_controller1,
        storage1,
        channels1,
        mip_store.clone(),
        metrics.clone(),
    )
    .expect("Failed to start protocol 1");
    let (mut manager2, _, _) = start_protocol_controller(
        config2,
        selector_controller2,
        consensus_controller2,
        None,
        pool_controller2,
        storage2,
        channels2,
        mip_store,
        metrics,
    )
    .expect("Failed to start protocol 2");

    std::thread::sleep(Duration::from_secs(15));
    // Stop the protocols
    sender_manager1.stop();
    sender_manager2.stop();
    manager1.stop();
    manager2.stop();
}
