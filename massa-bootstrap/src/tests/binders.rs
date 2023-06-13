use crate::messages::{BootstrapClientMessage, BootstrapServerMessage};
use crate::settings::{BootstrapClientConfig, BootstrapSrvBindCfg};
use crate::{
    bindings::{BootstrapClientBinder, BootstrapServerBinder},
    tests::tools::get_bootstrap_config,
    BootstrapPeers,
};
use crate::{BootstrapConfig, BootstrapError};
use massa_models::config::{
    BOOTSTRAP_RANDOMNESS_SIZE_BYTES, CONSENSUS_BOOTSTRAP_PART_SIZE, ENDORSEMENT_COUNT,
    MAX_ADVERTISE_LENGTH, MAX_ASYNC_MESSAGE_DATA, MAX_ASYNC_POOL_LENGTH,
    MAX_BOOTSTRAPPED_NEW_ELEMENTS, MAX_BOOTSTRAP_ASYNC_POOL_CHANGES, MAX_BOOTSTRAP_BLOCKS,
    MAX_BOOTSTRAP_ERROR_LENGTH, MAX_DATASTORE_ENTRY_COUNT, MAX_DATASTORE_KEY_LENGTH,
    MAX_DATASTORE_VALUE_LENGTH, MAX_DEFERRED_CREDITS_LENGTH, MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
    MAX_DENUNCIATION_CHANGES_LENGTH, MAX_EXECUTED_OPS_CHANGES_LENGTH, MAX_EXECUTED_OPS_LENGTH,
    MAX_LEDGER_CHANGES_COUNT, MAX_LISTENERS_PER_PEER, MAX_OPERATIONS_PER_BLOCK,
    MAX_PRODUCTION_STATS_LENGTH, MAX_ROLLS_COUNT_LENGTH, MIP_STORE_STATS_BLOCK_CONSIDERED,
    MIP_STORE_STATS_COUNTERS_MAX, THREAD_COUNT,
};
use massa_models::node::NodeId;
use massa_models::version::Version;
use massa_protocol_exports::{PeerId, TransportType};
use massa_signature::{KeyPair, PublicKey};
use massa_time::MassaTime;
use std::collections::HashMap;
use std::io::Write;
use std::net::TcpStream;
use std::str::FromStr;

lazy_static::lazy_static! {
    pub static ref BOOTSTRAP_CONFIG_KEYPAIR: (BootstrapConfig, KeyPair) = {
        let keypair = KeyPair::generate(0).unwrap();
        (get_bootstrap_config(NodeId::new(keypair.get_public_key())), keypair)
    };
}

impl BootstrapClientBinder {
    pub fn test_default(client_duplex: TcpStream, remote_pubkey: PublicKey) -> Self {
        let cfg = BootstrapClientConfig {
            max_bytes_read_write: f64::INFINITY,
            max_listeners_per_peer: MAX_LISTENERS_PER_PEER as u32,
            endorsement_count: ENDORSEMENT_COUNT,
            max_advertise_length: MAX_ADVERTISE_LENGTH,
            max_bootstrap_blocks_length: MAX_BOOTSTRAP_BLOCKS,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            thread_count: THREAD_COUNT,
            randomness_size_bytes: BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
            max_bootstrap_error_length: MAX_BOOTSTRAP_ERROR_LENGTH,
            max_new_elements: MAX_BOOTSTRAPPED_NEW_ELEMENTS,
            max_datastore_entry_count: MAX_DATASTORE_ENTRY_COUNT,
            max_datastore_key_length: MAX_DATASTORE_KEY_LENGTH,
            max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
            max_async_pool_changes: MAX_BOOTSTRAP_ASYNC_POOL_CHANGES,
            max_async_pool_length: MAX_ASYNC_POOL_LENGTH,
            max_async_message_data: MAX_ASYNC_MESSAGE_DATA,
            max_ledger_changes_count: MAX_LEDGER_CHANGES_COUNT,
            max_changes_slot_count: 1000,
            max_rolls_length: MAX_ROLLS_COUNT_LENGTH,
            max_production_stats_length: MAX_PRODUCTION_STATS_LENGTH,
            max_credits_length: MAX_DEFERRED_CREDITS_LENGTH,
            max_executed_ops_length: MAX_EXECUTED_OPS_LENGTH,
            max_ops_changes_length: MAX_EXECUTED_OPS_CHANGES_LENGTH,
            mip_store_stats_block_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            mip_store_stats_counters_max: MIP_STORE_STATS_COUNTERS_MAX,
            max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            max_denunciation_changes_length: MAX_DENUNCIATION_CHANGES_LENGTH,
        };
        BootstrapClientBinder::new(client_duplex, remote_pubkey, cfg)
    }
}

/// The server and the client will handshake and then send message in both ways in order
#[test]
fn test_binders() {
    let (bootstrap_config, server_keypair): &(BootstrapConfig, KeyPair) = &BOOTSTRAP_CONFIG_KEYPAIR;
    let server = std::net::TcpListener::bind("localhost:0").unwrap();
    let addr = server.local_addr().unwrap();
    let client = std::net::TcpStream::connect(addr).unwrap();
    let server = server.accept().unwrap();
    let version = || Version::from_str("TEST.1.10").unwrap();

    let mut server = BootstrapServerBinder::new(
        server.0,
        server_keypair.clone(),
        BootstrapSrvBindCfg {
            max_bytes_read_write: f64::INFINITY,
            thread_count: THREAD_COUNT,
            max_datastore_key_length: MAX_DATASTORE_KEY_LENGTH,
            randomness_size_bytes: BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
            consensus_bootstrap_part_size: CONSENSUS_BOOTSTRAP_PART_SIZE,
            write_error_timeout: MassaTime::from_millis(1000),
        },
    );
    let mut client = BootstrapClientBinder::test_default(
        client,
        bootstrap_config.bootstrap_list[0].1.get_public_key(),
    );

    let peer_id1 = PeerId::from_public_key(KeyPair::generate(0).unwrap().get_public_key());
    let peer_id2 = PeerId::from_public_key(KeyPair::generate(0).unwrap().get_public_key());
    let peer_id3 = PeerId::from_public_key(KeyPair::generate(0).unwrap().get_public_key());
    let peer_id4 = PeerId::from_public_key(KeyPair::generate(0).unwrap().get_public_key());

    let server_thread = std::thread::Builder::new()
        .name("test_binders::server_thread".to_string())
        .spawn({
            let peer_id1 = peer_id1.clone();
            let peer_id2 = peer_id2.clone();
            let peer_id3 = peer_id3.clone();
            let peer_id4 = peer_id4.clone();
            move || {
                // Test message 1
                let mut listeners = HashMap::default();
                listeners.insert(
                    bootstrap_config.bootstrap_list[0].0.clone(),
                    TransportType::Tcp,
                );
                let vector_peers = vec![(peer_id1, listeners)];
                let test_peers_message = BootstrapServerMessage::BootstrapPeers {
                    peers: BootstrapPeers(vector_peers.clone()),
                };

                server.handshake_timeout(version(), None).unwrap();

                server
                    .send_timeout(test_peers_message.clone(), None)
                    .unwrap();

                let message = server.next_timeout(None).unwrap();
                match message {
                    BootstrapClientMessage::BootstrapError { error } => {
                        assert_eq!(error, "test error");
                    }
                    _ => panic!("Bad message receive: Expected a peers list message"),
                }

                // Test message 3
                let mut listeners = HashMap::default();
                listeners.insert(
                    bootstrap_config.bootstrap_list[0].0.clone(),
                    TransportType::Tcp,
                );
                let vector_peers = vec![
                    (peer_id2, listeners.clone()),
                    (peer_id3, listeners.clone()),
                    (peer_id4, listeners.clone()),
                ];
                let test_peers_message = BootstrapServerMessage::BootstrapPeers {
                    peers: BootstrapPeers(vector_peers.clone()),
                };

                server
                    .send_timeout(test_peers_message.clone(), None)
                    .unwrap();
            }
        })
        .unwrap();

    let client_thread = std::thread::Builder::new()
        .name("test_binders::server_thread".to_string())
        .spawn({
            let peer_id1 = peer_id1.clone();
            let peer_id2 = peer_id2.clone();
            let peer_id3 = peer_id3.clone();
            let peer_id4 = peer_id4.clone();
            move || {
                // Test message 1
                let mut listeners = HashMap::default();
                listeners.insert(
                    bootstrap_config.bootstrap_list[0].0.clone(),
                    TransportType::Tcp,
                );
                let vector_peers = vec![(peer_id1, listeners)];

                client.handshake(version()).unwrap();
                let message = client.next_timeout(None).unwrap();
                match message {
                    BootstrapServerMessage::BootstrapPeers { peers } => {
                        assert_eq!(vector_peers, peers.0);
                    }
                    _ => panic!("Bad message receive: Expected a peers list message"),
                }

                client
                    .send_timeout(
                        &BootstrapClientMessage::BootstrapError {
                            error: "test error".to_string(),
                        },
                        None,
                    )
                    .unwrap();

                // Test message 3
                let mut listeners = HashMap::default();
                listeners.insert(
                    bootstrap_config.bootstrap_list[0].0.clone(),
                    TransportType::Tcp,
                );
                let vector_peers = vec![
                    (peer_id2, listeners.clone()),
                    (peer_id3, listeners.clone()),
                    (peer_id4, listeners.clone()),
                ];
                let message = client.next_timeout(None).unwrap();
                match message {
                    BootstrapServerMessage::BootstrapPeers { peers } => {
                        assert_eq!(vector_peers, peers.0);
                    }
                    _ => panic!("Bad message receive: Expected a peers list message"),
                }
            }
        })
        .unwrap();

    server_thread.join().unwrap();
    client_thread.join().unwrap();
}

/// The server and the client will handshake and then send message only from server to client
#[test]
fn test_binders_double_send_server_works() {
    let (bootstrap_config, server_keypair): &(BootstrapConfig, KeyPair) = &BOOTSTRAP_CONFIG_KEYPAIR;

    let server = std::net::TcpListener::bind("localhost:0").unwrap();
    let client = std::net::TcpStream::connect(server.local_addr().unwrap()).unwrap();
    let server = server.accept().unwrap();
    let version = || Version::from_str("TEST.1.10").unwrap();

    let mut server = BootstrapServerBinder::new(
        server.0,
        server_keypair.clone(),
        BootstrapSrvBindCfg {
            max_bytes_read_write: f64::INFINITY,
            thread_count: THREAD_COUNT,
            max_datastore_key_length: MAX_DATASTORE_KEY_LENGTH,
            randomness_size_bytes: BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
            consensus_bootstrap_part_size: CONSENSUS_BOOTSTRAP_PART_SIZE,
            write_error_timeout: MassaTime::from_millis(1000),
        },
    );
    let mut client = BootstrapClientBinder::test_default(
        client,
        bootstrap_config.bootstrap_list[0].1.get_public_key(),
    );

    let peer_id1 = PeerId::from_public_key(KeyPair::generate(0).unwrap().get_public_key());
    let peer_id2 = PeerId::from_public_key(KeyPair::generate(0).unwrap().get_public_key());
    let peer_id3 = PeerId::from_public_key(KeyPair::generate(0).unwrap().get_public_key());
    let peer_id4 = PeerId::from_public_key(KeyPair::generate(0).unwrap().get_public_key());

    let server_thread = std::thread::Builder::new()
        .name("test_buinders_double_send_server_works::server_thread".to_string())
        .spawn({
            let peer_id1 = peer_id1.clone();
            let peer_id2 = peer_id2.clone();
            let peer_id3 = peer_id3.clone();
            let peer_id4 = peer_id4.clone();
            move || {
                // Test message 1
                let mut listeners = HashMap::default();
                listeners.insert(
                    bootstrap_config.bootstrap_list[0].0.clone(),
                    TransportType::Tcp,
                );
                let vector_peers = vec![(peer_id1, listeners.clone())];
                let test_peers_message = BootstrapServerMessage::BootstrapPeers {
                    peers: BootstrapPeers(vector_peers.clone()),
                };

                server.handshake_timeout(version(), None).unwrap();
                server
                    .send_timeout(test_peers_message.clone(), None)
                    .unwrap();

                // Test message 2
                let mut listeners = HashMap::default();
                listeners.insert(
                    bootstrap_config.bootstrap_list[0].0.clone(),
                    TransportType::Tcp,
                );
                let vector_peers = vec![
                    (peer_id2, listeners.clone()),
                    (peer_id3, listeners.clone()),
                    (peer_id4, listeners.clone()),
                ];
                let test_peers_message = BootstrapServerMessage::BootstrapPeers {
                    peers: BootstrapPeers(vector_peers.clone()),
                };

                server
                    .send_timeout(test_peers_message.clone(), None)
                    .unwrap();
            }
        })
        .unwrap();

    let client_thread = std::thread::Builder::new()
        .name("test_buinders_double_send_server_works::client_thread".to_string())
        .spawn({
            let peer_id1 = peer_id1.clone();
            let peer_id2 = peer_id2.clone();
            let peer_id3 = peer_id3.clone();
            let peer_id4 = peer_id4.clone();
            move || {
                // Test message 1
                let mut listeners = HashMap::default();
                listeners.insert(
                    bootstrap_config.bootstrap_list[0].0.clone(),
                    TransportType::Tcp,
                );
                let vector_peers = vec![(peer_id1, listeners.clone())];

                client.handshake(version()).unwrap();
                let message = client.next_timeout(None).unwrap();
                match message {
                    BootstrapServerMessage::BootstrapPeers { peers } => {
                        assert_eq!(vector_peers, peers.0);
                    }
                    _ => panic!("Bad message receive: Expected a peers list message"),
                }

                // Test message 2
                let mut listeners = HashMap::default();
                listeners.insert(
                    bootstrap_config.bootstrap_list[0].0.clone(),
                    TransportType::Tcp,
                );
                let vector_peers = vec![
                    (peer_id2, listeners.clone()),
                    (peer_id3, listeners.clone()),
                    (peer_id4, listeners.clone()),
                ];
                let message = client.next_timeout(None).unwrap();
                match message {
                    BootstrapServerMessage::BootstrapPeers { peers } => {
                        assert_eq!(vector_peers, peers.0);
                    }
                    _ => panic!("Bad message receive: Expected a peers list message"),
                }
            }
        })
        .unwrap();

    server_thread.join().unwrap();
    client_thread.join().unwrap();
}

/// The server and the client will handshake and then send message in both ways but the client will try to send two messages without answer
#[test]
fn test_binders_try_double_send_client_works() {
    let (bootstrap_config, server_keypair): &(BootstrapConfig, KeyPair) = &BOOTSTRAP_CONFIG_KEYPAIR;

    let server = std::net::TcpListener::bind("localhost:0").unwrap();
    let addr = server.local_addr().unwrap();
    let client = std::net::TcpStream::connect(addr).unwrap();
    let server = server.accept().unwrap();
    let mut server = BootstrapServerBinder::new(
        server.0,
        server_keypair.clone(),
        BootstrapSrvBindCfg {
            max_bytes_read_write: f64::INFINITY,
            thread_count: THREAD_COUNT,
            max_datastore_key_length: MAX_DATASTORE_KEY_LENGTH,
            randomness_size_bytes: BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
            consensus_bootstrap_part_size: CONSENSUS_BOOTSTRAP_PART_SIZE,
            write_error_timeout: MassaTime::from_millis(1000),
        },
    );
    let mut client = BootstrapClientBinder::test_default(
        client,
        bootstrap_config.bootstrap_list[0].1.get_public_key(),
    );
    let version = || Version::from_str("TEST.1.10").unwrap();

    let peer_id1 = PeerId::from_public_key(KeyPair::generate(0).unwrap().get_public_key());

    let server_thread = std::thread::Builder::new()
        .name("test_buinders_double_send_client_works::server_thread".to_string())
        .spawn({
            let peer_id1 = peer_id1.clone();
            move || {
                // Test message 1
                let mut listeners = HashMap::default();
                listeners.insert(
                    bootstrap_config.bootstrap_list[0].0.clone(),
                    TransportType::Tcp,
                );
                let vector_peers = vec![(peer_id1, listeners.clone())];
                let test_peers_message = BootstrapServerMessage::BootstrapPeers {
                    peers: BootstrapPeers(vector_peers.clone()),
                };

                server.handshake_timeout(version(), None).unwrap();
                server
                    .send_timeout(test_peers_message.clone(), None)
                    .unwrap();

                let message = server.next_timeout(None).unwrap();
                match message {
                    BootstrapClientMessage::BootstrapError { error } => {
                        assert_eq!(error, "test error");
                    }
                    _ => panic!("Bad message receive: Expected a peers list message"),
                }

                let message = server.next_timeout(None).unwrap();
                match message {
                    BootstrapClientMessage::BootstrapError { error } => {
                        assert_eq!(error, "test error");
                    }
                    _ => panic!("Bad message receive: Expected a peers list message"),
                }

                server
                    .send_timeout(test_peers_message.clone(), None)
                    .unwrap();
            }
        })
        .unwrap();

    let client_thread = std::thread::Builder::new()
        .name("test_buinders_double_send_client_works::client_thread".to_string())
        .spawn({
            let peer_id1 = peer_id1.clone();
            move || {
                // Test message 1
                let mut listeners = HashMap::default();
                listeners.insert(
                    bootstrap_config.bootstrap_list[0].0.clone(),
                    TransportType::Tcp,
                );
                let vector_peers = vec![(peer_id1.clone(), listeners.clone())];

                client.handshake(version()).unwrap();
                let message = client.next_timeout(None).unwrap();
                match message {
                    BootstrapServerMessage::BootstrapPeers { peers } => {
                        assert_eq!(vector_peers, peers.0);
                    }
                    _ => panic!("Bad message receive: Expected a peers list message"),
                }

                client
                    .send_timeout(
                        &BootstrapClientMessage::BootstrapError {
                            error: "test error".to_string(),
                        },
                        None,
                    )
                    .unwrap();

                // Test message 3
                client
                    .send_timeout(
                        &BootstrapClientMessage::BootstrapError {
                            error: "test error".to_string(),
                        },
                        None,
                    )
                    .unwrap();

                let mut listeners = HashMap::default();
                listeners.insert(
                    bootstrap_config.bootstrap_list[0].0.clone(),
                    TransportType::Tcp,
                );
                let vector_peers = vec![(peer_id1, listeners.clone())];
                let message = client.next_timeout(None).unwrap();
                match message {
                    BootstrapServerMessage::BootstrapPeers { peers } => {
                        assert_eq!(vector_peers, peers.0);
                    }
                    _ => panic!("Bad message receive: Expected a peers list message"),
                }
            }
        })
        .unwrap();

    server_thread.join().unwrap();
    client_thread.join().unwrap();
}

#[test]
fn test_partial_msg() {
    let (bootstrap_config, server_keypair): &(BootstrapConfig, KeyPair) = &BOOTSTRAP_CONFIG_KEYPAIR;
    let server = std::net::TcpListener::bind("localhost:0").unwrap();
    let addr = server.local_addr().unwrap();
    let client = std::net::TcpStream::connect(addr).unwrap();
    let mut client_clone = client.try_clone().unwrap();
    let server = server.accept().unwrap();
    let version = || Version::from_str("TEST.1.10").unwrap();

    let mut server = BootstrapServerBinder::new(
        server.0,
        server_keypair.clone(),
        BootstrapSrvBindCfg {
            max_bytes_read_write: f64::INFINITY,
            thread_count: THREAD_COUNT,
            max_datastore_key_length: MAX_DATASTORE_KEY_LENGTH,
            randomness_size_bytes: BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
            consensus_bootstrap_part_size: CONSENSUS_BOOTSTRAP_PART_SIZE,
            write_error_timeout: MassaTime::from_millis(1000),
        },
    );
    let mut client = BootstrapClientBinder::test_default(
        client,
        bootstrap_config.bootstrap_list[0].1.get_public_key(),
    );

    let server_thread = std::thread::Builder::new()
        .name("test_binders::server_thread".to_string())
        .spawn({
            move || {
                server.handshake_timeout(version(), None).unwrap();

                let message = server.next_timeout(None).unwrap_err();
                match message {
                    BootstrapError::IoError(message) => {
                        assert_eq!(message.kind(), std::io::ErrorKind::UnexpectedEof);
                        assert_eq!(message.to_string(), "failed to fill whole buffer: 0/2");
                    }
                    _ => panic!("expected an io_error"),
                }
            }
        })
        .unwrap();

    let client_thread = std::thread::Builder::new()
        .name("test_binders::server_thread".to_string())
        .spawn({
            move || {
                client.handshake(version()).unwrap();

                // write the signature.
                // This test  assumes that the the signature is not checked until the message is read in
                // its entirety. The signature here would cause the message exchange to fail on that basis
                // if this assumption is broken.
                client_clone
                    .write_all(b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA")
                    .unwrap();
                // Give a non-zero message length, but never provide any msg-bytes
                client_clone.write_all(&[0, 0, 0, 2]).unwrap();
            }
        })
        .unwrap();

    server_thread.join().unwrap();
    client_thread.join().unwrap();
}
