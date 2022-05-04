use std::str::FromStr;

use super::tools::get_keys;
use crate::messages::{BootstrapMessageClient, BootstrapMessageServer};
use crate::BootstrapSettings;
use crate::{
    tests::tools::get_bootstrap_config, BootstrapClientBinder, BootstrapPeers,
    BootstrapServerBinder,
};
use massa_models::Version;
use massa_signature::PrivateKey;
use serial_test::serial;
use tokio::io::duplex;

lazy_static::lazy_static! {
    pub static ref BOOTSTRAP_SETTINGS_PRIVATE_KEY: (BootstrapSettings, PrivateKey) = {
        let (private_key, public_key) = get_keys();
        (get_bootstrap_config(public_key), private_key)
    };
}

/// The server and the client will handshake and then send message in both ways in order
#[tokio::test]
#[serial]
async fn test_binders() {
    let (bootstrap_settings, server_private_key): &(BootstrapSettings, PrivateKey) =
        &BOOTSTRAP_SETTINGS_PRIVATE_KEY;

    let (client, server) = duplex(1000000);
    let mut server = BootstrapServerBinder::new(server, *server_private_key);
    let mut client = BootstrapClientBinder::new(client, bootstrap_settings.bootstrap_list[0].1);

    let server_thread = tokio::spawn(async move {
        // Test message 1
        let vector_peers = vec![bootstrap_settings.bootstrap_list[0].0.ip()];
        let test_peers_message = BootstrapMessageServer::BootstrapPeers {
            peers: BootstrapPeers(vector_peers.clone()),
        };

        let version: Version = Version::from_str("TEST.1.2").unwrap();

        server.handshake(version).await.unwrap();
        server.send(test_peers_message.clone()).await.unwrap();

        let message = server.next().await.unwrap();
        match message {
            BootstrapMessageClient::BootstrapError { error } => {
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
        let test_peers_message = BootstrapMessageServer::BootstrapPeers {
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
            BootstrapMessageServer::BootstrapPeers { peers } => {
                assert_eq!(vector_peers, peers.0);
            }
            _ => panic!("Bad message receive: Expected a peers list message"),
        }

        client
            .send(BootstrapMessageClient::BootstrapError {
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
            BootstrapMessageServer::BootstrapPeers { peers } => {
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
    let (bootstrap_settings, server_private_key): &(BootstrapSettings, PrivateKey) =
        &BOOTSTRAP_SETTINGS_PRIVATE_KEY;

    let (client, server) = duplex(1000000);
    let mut server = BootstrapServerBinder::new(server, *server_private_key);
    let mut client = BootstrapClientBinder::new(client, bootstrap_settings.bootstrap_list[0].1);

    let server_thread = tokio::spawn(async move {
        // Test message 1
        let vector_peers = vec![bootstrap_settings.bootstrap_list[0].0.ip()];
        let test_peers_message = BootstrapMessageServer::BootstrapPeers {
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
        let test_peers_message = BootstrapMessageServer::BootstrapPeers {
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
            BootstrapMessageServer::BootstrapPeers { peers } => {
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
            BootstrapMessageServer::BootstrapPeers { peers } => {
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
    let (bootstrap_settings, server_private_key): &(BootstrapSettings, PrivateKey) =
        &BOOTSTRAP_SETTINGS_PRIVATE_KEY;

    let (client, server) = duplex(1000000);
    let mut server = BootstrapServerBinder::new(server, *server_private_key);
    let mut client = BootstrapClientBinder::new(client, bootstrap_settings.bootstrap_list[0].1);

    let server_thread = tokio::spawn(async move {
        // Test message 1
        let vector_peers = vec![bootstrap_settings.bootstrap_list[0].0.ip()];
        let test_peers_message = BootstrapMessageServer::BootstrapPeers {
            peers: BootstrapPeers(vector_peers.clone()),
        };
        let version: Version = Version::from_str("TEST.1.2").unwrap();

        server.handshake(version).await.unwrap();
        server.send(test_peers_message.clone()).await.unwrap();

        let message = server.next().await.unwrap();
        match message {
            BootstrapMessageClient::BootstrapError { error } => {
                assert_eq!(error, "test error");
            }
            _ => panic!("Bad message receive: Expected a peers list message"),
        }

        let message = server.next().await.unwrap();
        match message {
            BootstrapMessageClient::BootstrapError { error } => {
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
            BootstrapMessageServer::BootstrapPeers { peers } => {
                assert_eq!(vector_peers, peers.0);
            }
            _ => panic!("Bad message receive: Expected a peers list message"),
        }

        client
            .send(BootstrapMessageClient::BootstrapError {
                error: "test error".to_string(),
            })
            .await
            .unwrap();

        // Test message 3
        client
            .send(BootstrapMessageClient::BootstrapError {
                error: "test error".to_string(),
            })
            .await
            .unwrap();

        let vector_peers = vec![bootstrap_settings.bootstrap_list[0].0.ip()];
        let message = client.next().await.unwrap();
        match message {
            BootstrapMessageServer::BootstrapPeers { peers } => {
                assert_eq!(vector_peers, peers.0);
            }
            _ => panic!("Bad message receive: Expected a peers list message"),
        }
    });

    server_thread.await.unwrap();
    client_thread.await.unwrap();
}
