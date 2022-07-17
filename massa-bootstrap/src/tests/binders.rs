use std::str::FromStr;

use crate::messages::{BootstrapClientMessage, BootstrapServerMessage};
use crate::BootstrapSettings;
use crate::{
    client_binder::BootstrapClientBinder, server_binder::BootstrapServerBinder,
    tests::tools::get_bootstrap_config, BootstrapPeers,
};
use massa_models::Version;
use massa_signature::KeyPair;
use serial_test::serial;
use tokio::io::duplex;

lazy_static::lazy_static! {
    pub static ref BOOTSTRAP_SETTINGS_KEYPAIR: (BootstrapSettings, KeyPair) = {
        let keypair = KeyPair::generate();
        (get_bootstrap_config(keypair.get_public_key()), keypair)
    };
}

/// The server and the client will handshake and then send message in both ways in order
#[tokio::test]
#[serial]
async fn test_binders() {
    let (bootstrap_settings, server_keypair): &(BootstrapSettings, KeyPair) =
        &BOOTSTRAP_SETTINGS_KEYPAIR;
    let (client, server) = duplex(1000000);
    let mut server = BootstrapServerBinder::new(server, server_keypair.clone(), f64::INFINITY);
    let mut client = BootstrapClientBinder::new(
        client,
        bootstrap_settings.bootstrap_list[0].1,
        f64::INFINITY,
    );

    let server_thread = tokio::spawn(async move {
        // Test message 1
        let vector_peers = vec![bootstrap_settings.bootstrap_list[0].0.ip()];
        let test_peers_message = BootstrapServerMessage::BootstrapPeers {
            peers: BootstrapPeers(vector_peers.clone()),
        };

        let version: Version = Version::from_str("TEST.1.2").unwrap();

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
            bootstrap_settings.bootstrap_list[0].0.ip(),
            bootstrap_settings.bootstrap_list[0].0.ip(),
            bootstrap_settings.bootstrap_list[0].0.ip(),
        ];
        let test_peers_message = BootstrapServerMessage::BootstrapPeers {
            peers: BootstrapPeers(vector_peers.clone()),
        };

        server.send(test_peers_message.clone()).await.unwrap();
    });

    let client_thread = tokio::spawn(async move {
        // Test message 1
        let vector_peers = vec![bootstrap_settings.bootstrap_list[0].0.ip()];

        let version: Version = Version::from_str("TEST.1.2").unwrap();

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
            bootstrap_settings.bootstrap_list[0].0.ip(),
            bootstrap_settings.bootstrap_list[0].0.ip(),
            bootstrap_settings.bootstrap_list[0].0.ip(),
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
    let (bootstrap_settings, server_keypair): &(BootstrapSettings, KeyPair) =
        &BOOTSTRAP_SETTINGS_KEYPAIR;

    let (client, server) = duplex(1000000);
    let mut server = BootstrapServerBinder::new(server, server_keypair.clone(), f64::INFINITY);
    let mut client = BootstrapClientBinder::new(
        client,
        bootstrap_settings.bootstrap_list[0].1,
        f64::INFINITY,
    );

    let server_thread = tokio::spawn(async move {
        // Test message 1
        let vector_peers = vec![bootstrap_settings.bootstrap_list[0].0.ip()];
        let test_peers_message = BootstrapServerMessage::BootstrapPeers {
            peers: BootstrapPeers(vector_peers.clone()),
        };

        let version: Version = Version::from_str("TEST.1.2").unwrap();

        server.handshake(version).await.unwrap();
        server.send(test_peers_message.clone()).await.unwrap();

        // Test message 2
        let vector_peers = vec![
            bootstrap_settings.bootstrap_list[0].0.ip(),
            bootstrap_settings.bootstrap_list[0].0.ip(),
            bootstrap_settings.bootstrap_list[0].0.ip(),
        ];
        let test_peers_message = BootstrapServerMessage::BootstrapPeers {
            peers: BootstrapPeers(vector_peers.clone()),
        };

        server.send(test_peers_message.clone()).await.unwrap();
    });

    let client_thread = tokio::spawn(async move {
        // Test message 1
        let vector_peers = vec![bootstrap_settings.bootstrap_list[0].0.ip()];

        let version: Version = Version::from_str("TEST.1.2").unwrap();

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
            bootstrap_settings.bootstrap_list[0].0.ip(),
            bootstrap_settings.bootstrap_list[0].0.ip(),
            bootstrap_settings.bootstrap_list[0].0.ip(),
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
    let (bootstrap_settings, server_keypair): &(BootstrapSettings, KeyPair) =
        &BOOTSTRAP_SETTINGS_KEYPAIR;

    let (client, server) = duplex(1000000);
    let mut server = BootstrapServerBinder::new(server, server_keypair.clone(), f64::INFINITY);
    let mut client = BootstrapClientBinder::new(
        client,
        bootstrap_settings.bootstrap_list[0].1,
        f64::INFINITY,
    );

    let server_thread = tokio::spawn(async move {
        // Test message 1
        let vector_peers = vec![bootstrap_settings.bootstrap_list[0].0.ip()];
        let test_peers_message = BootstrapServerMessage::BootstrapPeers {
            peers: BootstrapPeers(vector_peers.clone()),
        };
        let version: Version = Version::from_str("TEST.1.2").unwrap();

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
        let vector_peers = vec![bootstrap_settings.bootstrap_list[0].0.ip()];
        let version: Version = Version::from_str("TEST.1.2").unwrap();

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

        let vector_peers = vec![bootstrap_settings.bootstrap_list[0].0.ip()];
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
