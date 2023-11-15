// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::universe_client::{BootstrapClientForeignControllers, BootstrapClientTestUniverse};
use super::universe_server::BootstrapServerTestUniverseBuilder;
use crate::BootstrapConfig;
use crate::BootstrapError;
use massa_models::amount::Amount;
use massa_models::bytecode::Bytecode;
use massa_models::datastore::Datastore;
use massa_models::{address::Address, node::NodeId};
use massa_signature::KeyPair;
use massa_test_framework::TestUniverse;
use serial_test::serial;
use std::path::PathBuf;

#[test]
#[serial]
fn test_bootstrap_not_whitelisted() {
    let port = 8069;
    let server_keypair = KeyPair::generate(0).unwrap();
    let mut bootstrap_server_config = BootstrapConfig::default();
    bootstrap_server_config.bootstrap_whitelist_path =
        PathBuf::from("../massa-node/base_config/bootstrap_whitelist.json");
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
    let server_universe = BootstrapServerTestUniverseBuilder::new()
        .set_accept_error(true)
        .build();
    drop(server_universe);
}
