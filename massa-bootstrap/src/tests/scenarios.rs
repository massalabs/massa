// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::universe_client::{BootstrapClientForeignControllers, BootstrapClientTestUniverse};
use super::universe_server::BootstrapServerTestUniverseBuilder;
use crate::BootstrapConfig;
use crate::BootstrapError;
use crate::BootstrapServerMessage;
use massa_models::amount::Amount;
use massa_models::bytecode::Bytecode;
use massa_models::datastore::Datastore;
use massa_models::{address::Address, node::NodeId};
use massa_signature::KeyPair;
use massa_test_framework::TestUniverse;
use serial_test::serial;
use std::collections::HashSet;
use std::fs::File;
use std::net::IpAddr;
use std::str::FromStr;
use tempfile::NamedTempFile;

#[test]
#[serial]
fn test_bootstrap_not_whitelisted() {
    let port = 8069;
    let server_keypair = KeyPair::generate(0).unwrap();

    let bootstrap_whitelist_file = NamedTempFile::new().expect("unable to create temp file");

    let ips_str = vec![
        "149.202.86.103",
        "149.202.89.125",
        "158.69.120.215",
        "158.69.23.120",
        "198.27.74.5",
        "51.75.60.228",
        "2001:41d0:1004:67::",
        "2001:41d0:a:7f7d::",
        "2001:41d0:602:21e4::",
    ];

    let mut bootstrap_whitelist = HashSet::new();
    for ip_str in ips_str {
        bootstrap_whitelist.insert(IpAddr::from_str(ip_str).unwrap());
    }

    serde_json::to_writer_pretty::<&File, HashSet<IpAddr>>(
        bootstrap_whitelist_file.as_file(),
        &bootstrap_whitelist,
    )
    .expect("unable to write bootstrap whitelist temp file");

    let bootstrap_server_config = BootstrapConfig {
        bootstrap_whitelist_path: bootstrap_whitelist_file.path().to_path_buf(),
        ..Default::default()
    };
    let server_universe = BootstrapServerTestUniverseBuilder::new()
        .set_port(port)
        .set_config(bootstrap_server_config)
        .set_keypair(&server_keypair)
        .build();
    let mut client_universe = BootstrapClientTestUniverse::new(
        BootstrapClientForeignControllers::new_with_mocks(),
        BootstrapConfig::default(),
    );
    match client_universe.launch_bootstrap(port, NodeId::new(server_keypair.get_public_key())) {
        Ok(()) => panic!("Bootstrap should have failed"),
        Err(BootstrapError::ReceivedError(err)) => {
            assert_eq!(err, String::from("IP 127.0.0.1 is not in the whitelist"))
        }
        Err(err) => panic!("Unexpected error: {:?}", err),
    }
    drop(server_universe);
}

#[test]
#[serial]
fn test_bootstrap_server() {
    let port = 8070;
    let server_keypair = KeyPair::generate(0).unwrap();
    let address = Address::from_public_key(&server_keypair.get_public_key());
    let server_universe = BootstrapServerTestUniverseBuilder::new()
        .set_port(port)
        .set_keypair(&server_keypair)
        .set_address_balance(&address, Amount::from_mantissa_scale(100, 0).unwrap())
        .set_bytecode(&address, Bytecode(vec![0x00, 0x01, 0x02, 0x03]))
        .set_datastore(&address, Datastore::default())
        // FUTURE: set_ledger_changes or add a slot param to methods like `set_address_balance`
        .build();
    let mut client_universe = BootstrapClientTestUniverse::new(
        BootstrapClientForeignControllers::new_with_mocks(),
        BootstrapConfig::default(),
    );
    client_universe
        .launch_bootstrap(port, NodeId::new(server_keypair.get_public_key()))
        .unwrap();
    client_universe.compare_database(server_universe.database.clone());
}

// Regression test for Issue #3932
#[test]
#[serial]
fn test_bootstrap_accept_err() {
    let port = 8071;
    let server_universe = BootstrapServerTestUniverseBuilder::new()
        .set_accept_error(true)
        .set_port(port)
        .build();
    drop(server_universe);
}

// Regression test for Issue #4520: a client may open a session with `AskBootstrapPeers` (an
// honest client resuming on another server does exactly that), but repeating it would let it
// camp on a bootstrap slot until the deadline without ever streaming any state.
// Each server universe serves a single session, hence the two servers.
#[test]
#[serial]
fn scenario_ask_peers_only() {
    let server_keypair = KeyPair::generate(0).unwrap();
    let node_id = NodeId::new(server_keypair.get_public_key());

    // asking once without any prior state streaming is legitimate: the client may have completed
    // the state with a previous server and only be missing the peers
    let port = 8072;
    let server_universe = BootstrapServerTestUniverseBuilder::new()
        .set_port(port)
        .set_keypair(&server_keypair)
        .build();
    let mut client_universe = BootstrapClientTestUniverse::new(
        BootstrapClientForeignControllers::new_with_mocks(),
        BootstrapConfig::default(),
    );
    match client_universe.launch_ask_peers_only(port, node_id, 1) {
        Ok(BootstrapServerMessage::BootstrapPeers { .. }) => {}
        other => panic!("the first peers request should be served, got: {:?}", other),
    }
    drop(server_universe);

    // asking twice in the same session is not something an honest client does, and must be refused
    let port = 8073;
    let server_universe = BootstrapServerTestUniverseBuilder::new()
        .set_port(port)
        .set_keypair(&server_keypair)
        .build();
    let mut client_universe = BootstrapClientTestUniverse::new(
        BootstrapClientForeignControllers::new_with_mocks(),
        BootstrapConfig::default(),
    );
    match client_universe.launch_ask_peers_only(port, node_id, 2) {
        Err(BootstrapError::ReceivedError(error)) => {
            assert!(
                error.contains("AskBootstrapPeers"),
                "the server should report the repeated request, got: {}",
                error
            );
        }
        other => panic!(
            "the second peers request should have been refused, got: {:?}",
            other
        ),
    }
    drop(server_universe);
}
