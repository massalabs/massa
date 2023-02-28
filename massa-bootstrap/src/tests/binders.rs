use crate::messages::{BootstrapClientMessage, BootstrapServerMessage};
use crate::settings::{BootstrapClientConfig, BootstrapSrvBindCfg};
use crate::types::Duplex;
use crate::BootstrapConfig;
use crate::{
    client_binder::BootstrapClientBinder, server_binder::BootstrapServerBinder,
    tests::tools::get_bootstrap_config, BootstrapPeers,
};
use massa_models::config::{
    BOOTSTRAP_RANDOMNESS_SIZE_BYTES, CONSENSUS_BOOTSTRAP_PART_SIZE, ENDORSEMENT_COUNT,
    MAX_ADVERTISE_LENGTH, MAX_ASYNC_MESSAGE_DATA, MAX_ASYNC_POOL_LENGTH,
    MAX_BOOTSTRAP_ASYNC_POOL_CHANGES, MAX_BOOTSTRAP_BLOCKS, MAX_BOOTSTRAP_ERROR_LENGTH,
    MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE, MAX_BOOTSTRAP_MESSAGE_SIZE, MAX_DATASTORE_ENTRY_COUNT,
    MAX_DATASTORE_KEY_LENGTH, MAX_DATASTORE_VALUE_LENGTH, MAX_DEFERRED_CREDITS_LENGTH,
    MAX_EXECUTED_OPS_CHANGES_LENGTH, MAX_EXECUTED_OPS_LENGTH, MAX_LEDGER_CHANGES_COUNT,
    MAX_OPERATIONS_PER_BLOCK, MAX_PRODUCTION_STATS_LENGTH, MAX_ROLLS_COUNT_LENGTH, THREAD_COUNT,
};
use massa_models::node::NodeId;
use massa_models::version::Version;
use massa_signature::{KeyPair, PublicKey};
use massa_time::MassaTime;
use serial_test::serial;
use std::str::FromStr;
use tokio::io::duplex;

lazy_static::lazy_static! {
    pub static ref BOOTSTRAP_CONFIG_KEYPAIR: (BootstrapConfig, KeyPair) = {
        let keypair = KeyPair::generate();
        (get_bootstrap_config(NodeId::new(keypair.get_public_key())), keypair)
    };
}

impl BootstrapClientBinder {
    pub fn test_default(client_duplex: Duplex, remote_pubkey: PublicKey) -> Self {
        let cfg = BootstrapClientConfig {
            max_bytes_read_write: f64::INFINITY,
            max_bootstrap_message_size: MAX_BOOTSTRAP_MESSAGE_SIZE,
            endorsement_count: ENDORSEMENT_COUNT,
            max_advertise_length: MAX_ADVERTISE_LENGTH,
            max_bootstrap_blocks_length: MAX_BOOTSTRAP_BLOCKS,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            thread_count: THREAD_COUNT,
            randomness_size_bytes: BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
            max_bootstrap_error_length: MAX_BOOTSTRAP_ERROR_LENGTH,
            max_bootstrap_final_state_parts_size: MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE,
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
        };
        BootstrapClientBinder::new(client_duplex, remote_pubkey, cfg)
    }
}

/// The server and the client will handshake and then send message in both ways in order
#[tokio::test]
#[serial]
async fn test_binders() {
    let (bootstrap_config, server_keypair): &(BootstrapConfig, KeyPair) = &BOOTSTRAP_CONFIG_KEYPAIR;
    let (client, server) = duplex(1000000);
    let mut server = BootstrapServerBinder::new(
        server,
        server_keypair.clone(),
        BootstrapSrvBindCfg {
            max_bytes_read_write: f64::INFINITY,
            max_bootstrap_message_size: MAX_BOOTSTRAP_MESSAGE_SIZE,
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

    let server_thread = tokio::spawn(async move {
        // Test message 1
        let vector_peers = vec![bootstrap_config.bootstrap_list[0].0.ip()];
        let test_peers_message = BootstrapServerMessage::BootstrapPeers {
            peers: BootstrapPeers(vector_peers.clone()),
        };

        let version: Version = Version::from_str("TEST.1.10").unwrap();

        server.handshake(version).await.unwrap();
        server.send(test_peers_message.clone()).await.unwrap();

        let message = server.next().await.unwrap();
        match message {
            BootstrapClientMessage::BootstrapError { error } => {
                assert_eq!(error, "test error");
            }
            _ => panic!("Bad message receive: Expected a peers list message"),
        }

        // Test message 3
        let vector_peers = vec![
            bootstrap_config.bootstrap_list[0].0.ip(),
            bootstrap_config.bootstrap_list[0].0.ip(),
            bootstrap_config.bootstrap_list[0].0.ip(),
        ];
        let test_peers_message = BootstrapServerMessage::BootstrapPeers {
            peers: BootstrapPeers(vector_peers.clone()),
        };

        server.send(test_peers_message.clone()).await.unwrap();
    });

    let client_thread = tokio::spawn(async move {
        // Test message 1
        let vector_peers = vec![bootstrap_config.bootstrap_list[0].0.ip()];

        let version: Version = Version::from_str("TEST.1.10").unwrap();

        client.handshake(version).await.unwrap();
        let message = client.next().await.unwrap();
        match message {
            BootstrapServerMessage::BootstrapPeers { peers } => {
                assert_eq!(vector_peers, peers.0);
            }
            _ => panic!("Bad message receive: Expected a peers list message"),
        }

        client
            .send(&BootstrapClientMessage::BootstrapError {
                error: "test error".to_string(),
            })
            .await
            .unwrap();

        // Test message 3
        let vector_peers = vec![
            bootstrap_config.bootstrap_list[0].0.ip(),
            bootstrap_config.bootstrap_list[0].0.ip(),
            bootstrap_config.bootstrap_list[0].0.ip(),
        ];
        let message = client.next().await.unwrap();
        match message {
            BootstrapServerMessage::BootstrapPeers { peers } => {
                assert_eq!(vector_peers, peers.0);
            }
            _ => panic!("Bad message receive: Expected a peers list message"),
        }
    });

    server_thread.await.unwrap();
    client_thread.await.unwrap();
}

/// The server and the client will handshake and then send message only from server to client
#[tokio::test]
#[serial]
async fn test_binders_double_send_server_works() {
    let (bootstrap_config, server_keypair): &(BootstrapConfig, KeyPair) = &BOOTSTRAP_CONFIG_KEYPAIR;

    let (client, server) = duplex(1000000);

    let mut server = BootstrapServerBinder::new(
        server,
        server_keypair.clone(),
        BootstrapSrvBindCfg {
            max_bytes_read_write: f64::INFINITY,
            max_bootstrap_message_size: MAX_BOOTSTRAP_MESSAGE_SIZE,
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

    let server_thread = tokio::spawn(async move {
        // Test message 1
        let vector_peers = vec![bootstrap_config.bootstrap_list[0].0.ip()];
        let test_peers_message = BootstrapServerMessage::BootstrapPeers {
            peers: BootstrapPeers(vector_peers.clone()),
        };

        let version: Version = Version::from_str("TEST.1.10").unwrap();

        server.handshake(version).await.unwrap();
        server.send(test_peers_message.clone()).await.unwrap();

        // Test message 2
        let vector_peers = vec![
            bootstrap_config.bootstrap_list[0].0.ip(),
            bootstrap_config.bootstrap_list[0].0.ip(),
            bootstrap_config.bootstrap_list[0].0.ip(),
        ];
        let test_peers_message = BootstrapServerMessage::BootstrapPeers {
            peers: BootstrapPeers(vector_peers.clone()),
        };

        server.send(test_peers_message.clone()).await.unwrap();
    });

    let client_thread = tokio::spawn(async move {
        // Test message 1
        let vector_peers = vec![bootstrap_config.bootstrap_list[0].0.ip()];

        let version: Version = Version::from_str("TEST.1.10").unwrap();

        client.handshake(version).await.unwrap();
        let message = client.next().await.unwrap();
        match message {
            BootstrapServerMessage::BootstrapPeers { peers } => {
                assert_eq!(vector_peers, peers.0);
            }
            _ => panic!("Bad message receive: Expected a peers list message"),
        }

        // Test message 2
        let vector_peers = vec![
            bootstrap_config.bootstrap_list[0].0.ip(),
            bootstrap_config.bootstrap_list[0].0.ip(),
            bootstrap_config.bootstrap_list[0].0.ip(),
        ];
        let message = client.next().await.unwrap();
        match message {
            BootstrapServerMessage::BootstrapPeers { peers } => {
                assert_eq!(vector_peers, peers.0);
            }
            _ => panic!("Bad message receive: Expected a peers list message"),
        }
    });

    server_thread.await.unwrap();
    client_thread.await.unwrap();
}

/// The server and the client will handshake and then send message in both ways but the client will try to send two messages without answer
#[tokio::test]
#[serial]
async fn test_binders_try_double_send_client_works() {
    let (bootstrap_config, server_keypair): &(BootstrapConfig, KeyPair) = &BOOTSTRAP_CONFIG_KEYPAIR;

    let (client, server) = duplex(1000000);
    let mut server = BootstrapServerBinder::new(
        server,
        server_keypair.clone(),
        BootstrapSrvBindCfg {
            max_bytes_read_write: f64::INFINITY,
            max_bootstrap_message_size: MAX_BOOTSTRAP_MESSAGE_SIZE,
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

    let server_thread = tokio::spawn(async move {
        // Test message 1
        let vector_peers = vec![bootstrap_config.bootstrap_list[0].0.ip()];
        let test_peers_message = BootstrapServerMessage::BootstrapPeers {
            peers: BootstrapPeers(vector_peers.clone()),
        };
        let version: Version = Version::from_str("TEST.1.10").unwrap();

        server.handshake(version).await.unwrap();
        server.send(test_peers_message.clone()).await.unwrap();

        let message = server.next().await.unwrap();
        match message {
            BootstrapClientMessage::BootstrapError { error } => {
                assert_eq!(error, "test error");
            }
            _ => panic!("Bad message receive: Expected a peers list message"),
        }

        let message = server.next().await.unwrap();
        match message {
            BootstrapClientMessage::BootstrapError { error } => {
                assert_eq!(error, "test error");
            }
            _ => panic!("Bad message receive: Expected a peers list message"),
        }

        server.send(test_peers_message.clone()).await.unwrap();
    });

    let client_thread = tokio::spawn(async move {
        // Test message 1
        let vector_peers = vec![bootstrap_config.bootstrap_list[0].0.ip()];
        let version: Version = Version::from_str("TEST.1.10").unwrap();

        client.handshake(version).await.unwrap();
        let message = client.next().await.unwrap();
        match message {
            BootstrapServerMessage::BootstrapPeers { peers } => {
                assert_eq!(vector_peers, peers.0);
            }
            _ => panic!("Bad message receive: Expected a peers list message"),
        }

        client
            .send(&BootstrapClientMessage::BootstrapError {
                error: "test error".to_string(),
            })
            .await
            .unwrap();

        // Test message 3
        client
            .send(&BootstrapClientMessage::BootstrapError {
                error: "test error".to_string(),
            })
            .await
            .unwrap();

        let vector_peers = vec![bootstrap_config.bootstrap_list[0].0.ip()];
        let message = client.next().await.unwrap();
        match message {
            BootstrapServerMessage::BootstrapPeers { peers } => {
                assert_eq!(vector_peers, peers.0);
            }
            _ => panic!("Bad message receive: Expected a peers list message"),
        }
    });

    server_thread.await.unwrap();
    client_thread.await.unwrap();
}
