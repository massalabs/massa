use std::time::Duration;

use massa_protocol_exports_2::ProtocolConfig;
use peernet::{peer_id::PeerId, transports::TransportType};
use tempfile::NamedTempFile;

use crate::{
    connectivity::{start_connectivity_thread, ConnectivityCommand},
    handlers::peer_handler::InitialPeers,
};

mod tools;

#[test]
fn basic() {
    // Setup the configs
    let mut config1 = ProtocolConfig::default();
    config1
        .listeners
        .insert("127.0.0.1:8081".parse().unwrap(), TransportType::Tcp);
    let mut config2 = ProtocolConfig::default();
    config2
        .listeners
        .insert("127.0.0.1:8082".parse().unwrap(), TransportType::Tcp);

    // Setup initial peers
    let initial_peers_file = NamedTempFile::new().expect("cannot create temp file");
    let mut initial_peers: InitialPeers = InitialPeers::default();
    initial_peers.insert(
        PeerId::from_public_key(config1.keypair.get_public_key()),
        config1.listeners.clone(),
    );
    serde_json::to_writer_pretty(initial_peers_file.as_file(), &initial_peers)
        .expect("unable to write ledger file");
    config1.initial_peers = initial_peers_file.path().to_path_buf();
    config2.initial_peers = initial_peers_file.path().to_path_buf();

    // Setup the protocols
    let (sender_manager1, _manager1) =
        start_connectivity_thread(config1).expect("Failed to start protocol 1");
    let (sender_manager2, _manager2) =
        start_connectivity_thread(config2).expect("Failed to start protocol 2");

    std::thread::sleep(Duration::from_secs(10));
    // Stop the protocols
    sender_manager1
        .send(ConnectivityCommand::Stop)
        .expect("Failed to send stop command to manager 1");
    sender_manager2
        .send(ConnectivityCommand::Stop)
        .expect("Failed to send stop command to manager 2");
    //TODO: Fix join are not working well
    // manager1.join().expect("Failed to join manager 1");
    // manager2.join().expect("Failed to join manager 2");
}
