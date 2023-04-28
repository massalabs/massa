use std::{collections::HashMap, fs::read_to_string, time::Duration};

use massa_consensus_exports::test_exports::ConsensusControllerImpl;
use massa_pool_exports::test_exports::MockPoolController;
use massa_protocol_exports::ProtocolConfig;
use massa_storage::Storage;
use peernet::{peer_id::PeerId, transports::TransportType, types::KeyPair};
use tempfile::NamedTempFile;

use crate::{
    create_protocol_controller, handlers::peer_handler::models::InitialPeers,
    start_protocol_controller,
};

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
    let mut initial_peers1: InitialPeers = InitialPeers::default();
    let mut peers_1 = HashMap::new();
    peers_1.insert("127.0.0.1:8082".parse().unwrap(), TransportType::Tcp);
    initial_peers1.insert(PeerId::from_public_key(keypair2.get_public_key()), peers_1);
    serde_json::to_writer_pretty(initial_peers_file.as_file(), &initial_peers1)
        .expect("unable to write ledger file");
    let initial_peers_file_2 = NamedTempFile::new().expect("cannot create temp file");
    let mut initial_peers2: InitialPeers = InitialPeers::default();
    let mut peers_2 = HashMap::new();
    peers_2.insert("127.0.0.1:8081".parse().unwrap(), TransportType::Tcp);
    initial_peers2.insert(PeerId::from_public_key(keypair1.get_public_key()), peers_2);
    serde_json::to_writer_pretty(initial_peers_file_2.as_file(), &initial_peers2)
        .expect("unable to write ledger file");
    config1.initial_peers = initial_peers_file.path().to_path_buf();
    config1.max_in_connections = 5;
    config1.max_out_connections = 1;
    config2.initial_peers = initial_peers_file_2.path().to_path_buf();
    config2.max_in_connections = 5;
    config2.max_out_connections = 0;
    config2.debug = false;

    // Setup the storages
    let storage1 = Storage::create_root();
    let storage2 = Storage::create_root();

    let (mut sender_manager1, channels1) = create_protocol_controller(config1.clone());
    let (mut sender_manager2, channels2) = create_protocol_controller(config2.clone());
    // Setup the protocols
    let (mut manager1, _, _) = start_protocol_controller(
        config1,
        consensus_controller1,
        pool_controller1,
        storage1,
        channels1,
    )
    .expect("Failed to start protocol 1");
    let (mut manager2, _, _) = start_protocol_controller(
        config2,
        consensus_controller2,
        pool_controller2,
        storage2,
        channels2,
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
        .insert("127.0.0.1:8084".parse().unwrap(), TransportType::Tcp);
    config2.keypair_file = "./src/tests/test_keypair1.json".to_string().into();
    let keypair_bs58_check_encoded = read_to_string(&config2.keypair_file)
        .map_err(|err| {
            std::io::Error::new(err.kind(), format!("could not load node key file: {}", err))
        })
        .unwrap();
    let keypair2 =
        serde_json::from_slice::<KeyPair>(keypair_bs58_check_encoded.as_bytes()).unwrap();

    // Setup initial peers
    let initial_peers_file = NamedTempFile::new().expect("cannot create temp file");
    let mut initial_peers1: InitialPeers = InitialPeers::default();
    let mut peers_1 = HashMap::new();
    peers_1.insert("127.0.0.1:8083".parse().unwrap(), TransportType::Tcp);
    initial_peers1.insert(PeerId::from_public_key(keypair2.get_public_key()), peers_1);
    serde_json::to_writer_pretty(initial_peers_file.as_file(), &initial_peers1)
        .expect("unable to write ledger file");
    let initial_peers_file_2 = NamedTempFile::new().expect("cannot create temp file");
    let mut initial_peers2: InitialPeers = InitialPeers::default();
    let mut peers_2 = HashMap::new();
    peers_2.insert("127.0.0.1:8084".parse().unwrap(), TransportType::Tcp);
    initial_peers2.insert(PeerId::from_public_key(keypair1.get_public_key()), peers_2);
    serde_json::to_writer_pretty(initial_peers_file_2.as_file(), &initial_peers2)
        .expect("unable to write ledger file");
    config1.initial_peers = initial_peers_file.path().to_path_buf();
    config1.max_in_connections = 5;
    config1.max_out_connections = 1;
    config2.initial_peers = initial_peers_file_2.path().to_path_buf();
    config2.max_in_connections = 5;
    config2.max_out_connections = 0;
    config2.debug = false;

    // Setup the storages
    let storage1 = Storage::create_root();
    let storage2 = Storage::create_root();

    // Setup the protocols
    let (mut sender_manager1, channels1) = create_protocol_controller(config1.clone());
    let (mut sender_manager2, channels2) = create_protocol_controller(config2.clone());
    let (mut manager1, _, _) = start_protocol_controller(
        config1,
        consensus_controller1,
        pool_controller1,
        storage1,
        channels1,
    )
    .expect("Failed to start protocol 1");
    let (mut manager2, _, _) = start_protocol_controller(
        config2,
        consensus_controller2,
        pool_controller2,
        storage2,
        channels2,
    )
    .expect("Failed to start protocol 2");

    std::thread::sleep(Duration::from_secs(15));
    // Stop the protocols
    sender_manager1.stop();
    sender_manager2.stop();
    manager1.stop();
    manager2.stop();
}
