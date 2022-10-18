use std::str::FromStr;

use crate::messages::{BootstrapClientMessage, BootstrapServerMessage};
use crate::BootstrapConfig;
use crate::{
    client_binder::BootstrapClientBinder, server_binder::BootstrapServerBinder,
    tests::tools::get_bootstrap_config, BootstrapPeers,
};
use massa_models::config::{
    BOOTSTRAP_RANDOMNESS_SIZE_BYTES, ENDORSEMENT_COUNT, MAX_ADVERTISE_LENGTH,
    MAX_ASYNC_MESSAGE_DATA, MAX_ASYNC_POOL_LENGTH, MAX_BOOTSTRAP_ASYNC_POOL_CHANGES,
    MAX_BOOTSTRAP_BLOCKS, MAX_BOOTSTRAP_CREDITS_LENGTH, MAX_BOOTSTRAP_ERROR_LENGTH,
    MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE, MAX_BOOTSTRAP_MESSAGE_SIZE,
    MAX_BOOTSTRAP_PRODUCTION_STATS, MAX_BOOTSTRAP_ROLLS_LENGTH, MAX_DATASTORE_ENTRY_COUNT,
    MAX_DATASTORE_KEY_LENGTH, MAX_DATASTORE_VALUE_LENGTH, MAX_FUNCTION_NAME_LENGTH,
    MAX_LEDGER_CHANGES_COUNT, MAX_OPERATIONS_PER_BLOCK, MAX_OPERATION_DATASTORE_ENTRY_COUNT,
    MAX_OPERATION_DATASTORE_KEY_LENGTH, MAX_OPERATION_DATASTORE_VALUE_LENGTH, MAX_PARAMETERS_SIZE,
    THREAD_COUNT,
};
use massa_models::version::Version;
use massa_signature::KeyPair;
use serial_test::serial;
use tokio::io::duplex;

lazy_static::lazy_static! {
    pub static ref BOOTSTRAP_CONFIG_KEYPAIR: (BootstrapConfig, KeyPair) = {
        let keypair = KeyPair::generate();
        (get_bootstrap_config(keypair.get_public_key()), keypair)
    };
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
        f64::INFINITY,
        MAX_BOOTSTRAP_MESSAGE_SIZE,
        THREAD_COUNT,
        MAX_DATASTORE_KEY_LENGTH,
        BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
    );
    let mut client = BootstrapClientBinder::new(
        client,
        bootstrap_config.bootstrap_list[0].1,
        f64::INFINITY,
        MAX_BOOTSTRAP_MESSAGE_SIZE,
        ENDORSEMENT_COUNT,
        MAX_ADVERTISE_LENGTH,
        MAX_BOOTSTRAP_BLOCKS,
        MAX_OPERATIONS_PER_BLOCK,
        THREAD_COUNT,
        BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
        MAX_BOOTSTRAP_ERROR_LENGTH,
        MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE,
        MAX_DATASTORE_ENTRY_COUNT,
        MAX_DATASTORE_KEY_LENGTH,
        MAX_DATASTORE_VALUE_LENGTH,
        MAX_BOOTSTRAP_ASYNC_POOL_CHANGES,
        MAX_ASYNC_POOL_LENGTH,
        MAX_ASYNC_MESSAGE_DATA,
        MAX_FUNCTION_NAME_LENGTH,
        MAX_PARAMETERS_SIZE,
        MAX_LEDGER_CHANGES_COUNT,
        MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        MAX_OPERATION_DATASTORE_KEY_LENGTH,
        MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        1000,
        MAX_BOOTSTRAP_ROLLS_LENGTH,
        MAX_BOOTSTRAP_PRODUCTION_STATS,
        MAX_BOOTSTRAP_CREDITS_LENGTH,
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
        f64::INFINITY,
        MAX_BOOTSTRAP_MESSAGE_SIZE,
        THREAD_COUNT,
        MAX_DATASTORE_KEY_LENGTH,
        BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
    );
    let mut client = BootstrapClientBinder::new(
        client,
        bootstrap_config.bootstrap_list[0].1,
        f64::INFINITY,
        MAX_BOOTSTRAP_MESSAGE_SIZE,
        ENDORSEMENT_COUNT,
        MAX_ADVERTISE_LENGTH,
        MAX_BOOTSTRAP_BLOCKS,
        MAX_OPERATIONS_PER_BLOCK,
        THREAD_COUNT,
        BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
        MAX_BOOTSTRAP_ERROR_LENGTH,
        MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE,
        MAX_DATASTORE_ENTRY_COUNT,
        MAX_DATASTORE_KEY_LENGTH,
        MAX_DATASTORE_VALUE_LENGTH,
        MAX_BOOTSTRAP_ASYNC_POOL_CHANGES,
        MAX_ASYNC_POOL_LENGTH,
        MAX_ASYNC_MESSAGE_DATA,
        MAX_FUNCTION_NAME_LENGTH,
        MAX_PARAMETERS_SIZE,
        MAX_LEDGER_CHANGES_COUNT,
        MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        MAX_OPERATION_DATASTORE_KEY_LENGTH,
        MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        1000,
        MAX_BOOTSTRAP_ROLLS_LENGTH,
        MAX_BOOTSTRAP_PRODUCTION_STATS,
        MAX_BOOTSTRAP_CREDITS_LENGTH,
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
        f64::INFINITY,
        MAX_BOOTSTRAP_MESSAGE_SIZE,
        THREAD_COUNT,
        MAX_DATASTORE_KEY_LENGTH,
        BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
    );
    let mut client = BootstrapClientBinder::new(
        client,
        bootstrap_config.bootstrap_list[0].1,
        f64::INFINITY,
        MAX_BOOTSTRAP_MESSAGE_SIZE,
        ENDORSEMENT_COUNT,
        MAX_ADVERTISE_LENGTH,
        MAX_BOOTSTRAP_BLOCKS,
        MAX_OPERATIONS_PER_BLOCK,
        THREAD_COUNT,
        BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
        MAX_BOOTSTRAP_ERROR_LENGTH,
        MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE,
        MAX_DATASTORE_ENTRY_COUNT,
        MAX_DATASTORE_KEY_LENGTH,
        MAX_DATASTORE_VALUE_LENGTH,
        MAX_BOOTSTRAP_ASYNC_POOL_CHANGES,
        MAX_ASYNC_POOL_LENGTH,
        MAX_ASYNC_MESSAGE_DATA,
        MAX_FUNCTION_NAME_LENGTH,
        MAX_PARAMETERS_SIZE,
        MAX_LEDGER_CHANGES_COUNT,
        MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        MAX_OPERATION_DATASTORE_KEY_LENGTH,
        MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        1000,
        MAX_BOOTSTRAP_ROLLS_LENGTH,
        MAX_BOOTSTRAP_PRODUCTION_STATS,
        MAX_BOOTSTRAP_CREDITS_LENGTH,
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
