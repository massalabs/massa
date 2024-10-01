use crate::messages::{BootstrapClientMessage, BootstrapServerMessage};
use crate::server::manage_bootstrap;
use crate::settings::{BootstrapClientConfig, BootstrapSrvBindCfg};

use crate::{
    bindings::{BootstrapClientBinder, BootstrapServerBinder},
    tests::tools::get_bootstrap_config,
};
use crate::{BootstrapConfig, BootstrapError};
use massa_consensus_exports::MockConsensusController;
use massa_db_exports::{MassaDBConfig, MassaDBController};
use massa_db_worker::MassaDB;
use massa_final_state::FinalStateConfig;
use massa_models::config::{
    BOOTSTRAP_RANDOMNESS_SIZE_BYTES, CHAINID, CONSENSUS_BOOTSTRAP_PART_SIZE, ENDORSEMENT_COUNT,
    MAX_ADVERTISE_LENGTH, MAX_BOOTSTRAP_BLOCKS, MAX_BOOTSTRAP_ERROR_LENGTH,
    MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE, MAX_BOOTSTRAP_VERSIONING_ELEMENTS_SIZE,
    MAX_DATASTORE_ENTRY_COUNT, MAX_DATASTORE_KEY_LENGTH, MAX_DATASTORE_VALUE_LENGTH,
    MAX_DEFERRED_CREDITS_LENGTH, MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
    MAX_DENUNCIATION_CHANGES_LENGTH, MAX_EXECUTED_OPS_CHANGES_LENGTH, MAX_EXECUTED_OPS_LENGTH,
    MAX_LEDGER_CHANGES_COUNT, MAX_LISTENERS_PER_PEER, MAX_OPERATIONS_PER_BLOCK,
    MAX_PRODUCTION_STATS_LENGTH, MAX_ROLLS_COUNT_LENGTH, MIP_STORE_STATS_BLOCK_CONSIDERED,
    THREAD_COUNT,
};
use massa_models::node::NodeId;
use massa_models::version::Version;

use massa_pos_exports::{MockSelectorControllerWrapper, PoSFinalState};
use massa_protocol_exports::{BootstrapPeers, MockProtocolController};
use massa_signature::{KeyPair, PublicKey};
use massa_time::MassaTime;

use parking_lot::RwLock;
use rand::Rng;
use serial_test::serial;
use tempfile::TempDir;

use std::io::Write;
use std::net::TcpStream;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::tools::{get_random_final_state_bootstrap, parametric_test};

lazy_static::lazy_static! {
    pub static ref BOOTSTRAP_CONFIG_KEYPAIR: (BootstrapConfig, KeyPair) = {
        let keypair = KeyPair::generate(0).unwrap();
        (get_bootstrap_config(NodeId::new(keypair.get_public_key())), keypair)
    };
}

impl BootstrapClientBinder {
    pub fn test_default(client_duplex: TcpStream, remote_pubkey: PublicKey) -> Self {
        let cfg = Self::test_default_config();
        BootstrapClientBinder::new(client_duplex, remote_pubkey, cfg, None)
    }
    pub(crate) fn test_default_config() -> BootstrapClientConfig {
        BootstrapClientConfig {
            rate_limit: u64::MAX,
            max_listeners_per_peer: MAX_LISTENERS_PER_PEER as u32,
            endorsement_count: ENDORSEMENT_COUNT,
            max_advertise_length: MAX_ADVERTISE_LENGTH,
            max_bootstrap_blocks_length: MAX_BOOTSTRAP_BLOCKS,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            thread_count: THREAD_COUNT,
            randomness_size_bytes: BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
            max_bootstrap_error_length: MAX_BOOTSTRAP_ERROR_LENGTH,
            max_final_state_elements_size: MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE,
            max_versioning_elements_size: MAX_BOOTSTRAP_VERSIONING_ELEMENTS_SIZE,
            max_datastore_entry_count: MAX_DATASTORE_ENTRY_COUNT,
            max_datastore_key_length: MAX_DATASTORE_KEY_LENGTH,
            max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
            max_ledger_changes_count: MAX_LEDGER_CHANGES_COUNT,
            max_changes_slot_count: 1000,
            max_rolls_length: MAX_ROLLS_COUNT_LENGTH,
            max_production_stats_length: MAX_PRODUCTION_STATS_LENGTH,
            max_credits_length: MAX_DEFERRED_CREDITS_LENGTH,
            max_executed_ops_length: MAX_EXECUTED_OPS_LENGTH,
            max_ops_changes_length: MAX_EXECUTED_OPS_CHANGES_LENGTH,
            mip_store_stats_block_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            max_denunciation_changes_length: MAX_DENUNCIATION_CHANGES_LENGTH,
            chain_id: *CHAINID,
        }
    }
}

// Get a message from the remote client, and assert it's the same as the one provided in argument
fn assert_server_got_msg(
    timeout: Duration,
    server: &mut BootstrapServerBinder,
    msg: BootstrapClientMessage,
) {
    let message = server.next_timeout(Some(timeout)).unwrap();
    let eq = message.equals(&msg);
    if !eq {
        println!("Got {message:?}");
        println!("Expected {msg:?}");
    }
    assert!(eq, "Received BootstrapClientMessage isn't the same");
}

// Get a message from the remote server, and assert it's the same as the one provided in argument
fn assert_client_got_msg(
    timeout: Duration,
    client: &mut BootstrapClientBinder,
    msg: BootstrapServerMessage,
) {
    let message = client.next_timeout(Some(timeout)).unwrap();
    let eq = message.equals(&msg);
    if !eq {
        println!("Got {message:?}");
        println!("Expected {msg:?}");
    }
    assert!(eq, "Received BootstrapServerMessage isn't the same");
}

// Initialize a pair of bootstrap server and client with the given configuration, and handshake done
fn init_server_client_pair() -> (BootstrapServerBinder, BootstrapClientBinder) {
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
            rate_limit: u64::MAX,
            thread_count: THREAD_COUNT,
            max_datastore_key_length: MAX_DATASTORE_KEY_LENGTH,
            randomness_size_bytes: BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
            consensus_bootstrap_part_size: CONSENSUS_BOOTSTRAP_PART_SIZE,
            write_error_timeout: MassaTime::from_millis(1000),
        },
        Some(u64::MAX),
    );
    let mut client = BootstrapClientBinder::test_default(
        client,
        bootstrap_config.bootstrap_list[0].1.get_public_key(),
    );
    client.handshake(version()).unwrap();
    server.handshake_timeout(version(), None).unwrap();

    (server, client)
}

/// The server and the client will handshake and then send message in both ways in order
// How this test works:
// - A "test controller" (closure inside the parametric_test function) will feed 2 messages for
//    each thread, one to send, one to receive
// - A server and client thread is started and listen to the channel to get the messages to send / receive
// - Each loop, the server will send a message, then listen for a message from the client,
//    once it gets it, it'll check it's the same as the one he's expecting
// - Same for the client thread
#[test]
fn test_binders_simple() {
    type Data = (BootstrapServerMessage, BootstrapClientMessage); // Sugar
    let timeout = Duration::from_secs(30);

    // Initialize the tests
    let (mut server, mut client) = init_server_client_pair();

    let (srv_send, srv_recv) = channel::<Data>();
    let (cli_send, cli_recv) = channel::<Data>();
    // Channels used to tell the controller that a test is finished (useful for thread sync)
    let (srv_ready_flag, srv_ready) = channel();
    let (cli_ready_flag, cli_ready) = channel();

    // Build the server thread
    let server_thread = std::thread::Builder::new()
        .name("test_binders_remake::server_thread".to_string())
        .spawn(move || loop {
            // So that the controller can wait on this thread to reach a new loop
            srv_ready_flag.send(true).unwrap();
            // Get the message to send, and the message to get, from the controller.
            let (srv_send_msg, cli_recv_msg) = match srv_recv.recv_timeout(timeout) {
                Ok(data) => data,
                Err(RecvTimeoutError::Timeout) => panic!("Timeout while waiting for message"),
                Err(RecvTimeoutError::Disconnected) => break,
            };
            // Send the message to the client
            server.send_timeout(srv_send_msg, Some(timeout)).unwrap();
            // Receive the message from the client, and assert its content is correct
            assert_server_got_msg(timeout, &mut server, cli_recv_msg);
        })
        .unwrap();

    let client_thread = std::thread::Builder::new()
        .name("test_binders_remake::client_thread".to_string())
        // Same principle as the server_thread above, only reversed
        .spawn(move || loop {
            cli_ready_flag.send(true).unwrap();
            let (srv_recv_msg, cli_send_msg) = match cli_recv.recv_timeout(timeout) {
                Ok(data) => data,
                Err(RecvTimeoutError::Timeout) => panic!("Timeout while waiting for message"),
                Err(RecvTimeoutError::Disconnected) => break,
            };
            assert_client_got_msg(timeout, &mut client, srv_recv_msg);
            client.send_timeout(&cli_send_msg, Some(timeout)).unwrap();
        })
        .unwrap();

    // Expects the loops to start before the first iteration of the tests
    assert_eq!(
        srv_ready.recv_timeout(timeout * 2),
        Ok(true),
        "Error while init server"
    );
    assert_eq!(
        cli_ready.recv_timeout(timeout * 2),
        Ok(true),
        "Error while init client"
    );

    // Parametric test function wrapper, see its definition for details
    parametric_test(
        Duration::from_secs(30),
        (),
        // Regressions already encountered
        vec![11687948547956751531, 6417627360786923628],
        move |_, rng| {
            // Generate random messages
            let server_message = BootstrapServerMessage::generate(rng);
            let client_message = BootstrapClientMessage::generate(rng);

            // Send the messages to the threads
            srv_send
                .send((server_message.clone(), client_message.clone()))
                .unwrap();
            cli_send.send((server_message, client_message)).unwrap();

            // Wait for the threads to finish
            assert_eq!(cli_ready.recv_timeout(timeout * 2), Ok(true));
            assert_eq!(srv_ready.recv_timeout(timeout * 2), Ok(true));
        },
    );

    let _ = server_thread.join();
    let _ = client_thread.join();
}

// This test uses exactly the same principle as the `test_binders_simple` one
// Except instead of passing a pair of (ServerMessage, ClientMessage), it will pass a
//    (bool, Vec<ServerMessage>, Vec<ClientMessage>)
// - The boolean defines whether the server or the client will transmit data first, or receive first
// - The first vector is a list of server messages generated that the server has to send
// - The second vector is a list of client messages generated that the client has to send
// Because the direction of the first message is randomly assigned, and the number of messages are random,
//     it will create a scenario of "Client sends 3 msg, Server sends 2 msg, Server sends 6 msg, Client sends 4 msg, etc ..."
#[test]
fn test_binders_multiple_send() {
    type Data = (
        bool,
        Vec<BootstrapServerMessage>,
        Vec<BootstrapClientMessage>,
    );
    let timeout = Duration::from_secs(10);
    let (mut server, mut client) = init_server_client_pair();

    let (srv_send, srv_recv) = channel::<Data>();
    let (cli_send, cli_recv) = channel::<Data>();
    let (srv_ready_flag, srv_ready) = channel();
    let (cli_ready_flag, cli_ready) = channel();

    let server_thread = std::thread::Builder::new()
        .name("test_binders_remake::server_thread".to_string())
        .spawn(move || loop {
            srv_ready_flag.send(true).unwrap();
            let data = match srv_recv.recv_timeout(timeout) {
                Ok(data) => data,
                Err(RecvTimeoutError::Timeout) => panic!("Timeout while waiting for message"),
                Err(RecvTimeoutError::Disconnected) => break,
            };
            if data.0 {
                for msg in data.1 {
                    server.send_timeout(msg, Some(timeout)).unwrap();
                }
                for msg in data.2 {
                    assert_server_got_msg(timeout, &mut server, msg);
                }
            } else {
                for msg in data.2 {
                    assert_server_got_msg(timeout, &mut server, msg);
                }
                for msg in data.1 {
                    server.send_timeout(msg, Some(timeout)).unwrap();
                }
            }
        })
        .unwrap();

    let client_thread = std::thread::Builder::new()
        .name("test_binders_remake::client_thread".to_string())
        .spawn(move || loop {
            cli_ready_flag.send(true).unwrap();
            let data = match cli_recv.recv_timeout(timeout) {
                Ok(data) => data,
                Err(RecvTimeoutError::Timeout) => panic!("Timeout while waiting for message"),
                Err(RecvTimeoutError::Disconnected) => break,
            };
            if data.0 {
                for msg in data.1 {
                    assert_client_got_msg(timeout, &mut client, msg);
                }
                for msg in data.2 {
                    client.send_timeout(&msg, Some(timeout)).unwrap();
                }
            } else {
                for msg in data.2 {
                    client.send_timeout(&msg, Some(timeout)).unwrap();
                }
                for msg in data.1 {
                    assert_client_got_msg(timeout, &mut client, msg);
                }
            }
        })
        .unwrap();

    assert_eq!(srv_ready.recv(), Ok(true), "Error while init server");
    assert_eq!(cli_ready.recv(), Ok(true), "Error while init client");
    parametric_test(Duration::from_secs(30), (), vec![], move |_, rng| {
        let direction = rng.gen_bool(0.5);
        let server_messages: Vec<BootstrapServerMessage> = (0..rng.gen_range(0..10))
            .map(|_| BootstrapServerMessage::generate(rng))
            .collect();
        let client_messages: Vec<BootstrapClientMessage> = (0..rng.gen_range(0..10))
            .map(|_| BootstrapClientMessage::generate(rng))
            .collect();
        srv_send
            .send((direction, server_messages.clone(), client_messages.clone()))
            .unwrap();
        cli_send
            .send((direction, server_messages, client_messages))
            .unwrap();
        assert_eq!(cli_ready.recv_timeout(timeout * 2), Ok(true));
        assert_eq!(srv_ready.recv_timeout(timeout * 2), Ok(true));
    });
    let _ = server_thread.join();
    let _ = client_thread.join();
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
            rate_limit: u64::MAX,
            thread_count: THREAD_COUNT,
            max_datastore_key_length: MAX_DATASTORE_KEY_LENGTH,
            randomness_size_bytes: BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
            consensus_bootstrap_part_size: CONSENSUS_BOOTSTRAP_PART_SIZE,
            write_error_timeout: MassaTime::from_millis(1000),
        },
        None,
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

#[test]
fn test_staying_connected_without_message_trigger_read_timeout() {
    let read_timeout_number = 1000;
    let read_timeout = Duration::from_millis(read_timeout_number);
    let (mut bootstrap_config, server_keypair): (BootstrapConfig, KeyPair) =
        BOOTSTRAP_CONFIG_KEYPAIR.clone();
    bootstrap_config.read_timeout = MassaTime::from_millis(read_timeout_number);
    let server = std::net::TcpListener::bind("localhost:0").unwrap();
    let addr = server.local_addr().unwrap();
    let client = std::net::TcpStream::connect(addr).unwrap();
    let mut client_clone = client.try_clone().unwrap();
    let server = server.accept().unwrap();
    let version = || Version::from_str("TEST.1.10").unwrap();
    let timeout: Duration = Duration::from_millis(10000);
    let consensus_controller = MockConsensusController::new();
    let protocol_controller = MockProtocolController::new();
    let selector_controller = MockSelectorControllerWrapper::new();

    let mut server = BootstrapServerBinder::new(
        server.0,
        server_keypair,
        BootstrapSrvBindCfg {
            rate_limit: u64::MAX,
            thread_count: THREAD_COUNT,
            max_datastore_key_length: MAX_DATASTORE_KEY_LENGTH,
            randomness_size_bytes: BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
            consensus_bootstrap_part_size: CONSENSUS_BOOTSTRAP_PART_SIZE,
            write_error_timeout: MassaTime::from_millis(1000),
        },
        None,
    );
    let temp_dir = TempDir::new().unwrap();
    let db = Arc::new(RwLock::new(Box::new(MassaDB::new(MassaDBConfig {
        path: temp_dir.path().to_path_buf(),
        max_history_length: 10,
        max_final_state_elements_size: 100_000_000,
        max_versioning_elements_size: 100_000_000,
        thread_count: THREAD_COUNT,
        max_ledger_backups: 10,
    }))
        as Box<(dyn MassaDBController + 'static)>));
    let rolls_path = PathBuf::from_str("../massa-node/base_config/initial_rolls.json").unwrap();
    let final_state_config = FinalStateConfig::default();
    let server_thread = std::thread::Builder::new()
        .name("test_binders::server_thread".to_string())
        .spawn({
            move || {
                manage_bootstrap(
                    &bootstrap_config,
                    &mut server,
                    Arc::new(RwLock::new(get_random_final_state_bootstrap(
                        PoSFinalState::new(
                            final_state_config.pos_config.clone(),
                            "",
                            &rolls_path,
                            Box::new(selector_controller.clone()),
                            db.clone(),
                        )
                        .unwrap(),
                        final_state_config,
                        db,
                    ))),
                    version(),
                    Box::new(consensus_controller),
                    Box::new(protocol_controller),
                    Instant::now().checked_add(timeout).unwrap(),
                )
            }
        })
        .unwrap();

    let client_thread = std::thread::Builder::new()
        .name("test_binders::server_thread".to_string())
        .spawn({
            move || {
                std::thread::sleep(
                    read_timeout
                        .checked_add(Duration::from_millis(100))
                        .unwrap(),
                );
                client_clone
                    .write_all(b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA")
                    .unwrap()
            }
        })
        .unwrap();

    let res = server_thread.join().unwrap();
    match res {
        Err(BootstrapError::TimedOut(_)) => (),
        _ => panic!("The server should have been interrupted"),
    }
    client_thread.join().unwrap();
}

#[test]
fn test_staying_connected_pass_handshake_but_deadline_after() {
    let read_timeout_number = 1000;
    let (mut bootstrap_config, server_keypair): (BootstrapConfig, KeyPair) =
        BOOTSTRAP_CONFIG_KEYPAIR.clone();
    bootstrap_config.read_timeout = MassaTime::from_millis(read_timeout_number);
    let server = std::net::TcpListener::bind("localhost:0").unwrap();
    let addr = server.local_addr().unwrap();
    let client = std::net::TcpStream::connect(addr).unwrap();
    let mut client_clone = client.try_clone().unwrap();
    let server = server.accept().unwrap();
    let version = || Version::from_str("TEST.1.10").unwrap();
    let timeout: Duration = Duration::from_millis(10);
    let consensus_controller = MockConsensusController::new();
    let protocol_controller = MockProtocolController::new();
    let selector_controller = MockSelectorControllerWrapper::new();

    let mut server = BootstrapServerBinder::new(
        server.0,
        server_keypair,
        BootstrapSrvBindCfg {
            rate_limit: u64::MAX,
            thread_count: THREAD_COUNT,
            max_datastore_key_length: MAX_DATASTORE_KEY_LENGTH,
            randomness_size_bytes: BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
            consensus_bootstrap_part_size: CONSENSUS_BOOTSTRAP_PART_SIZE,
            write_error_timeout: MassaTime::from_millis(1000),
        },
        None,
    );
    let mut client = BootstrapClientBinder::test_default(
        client,
        bootstrap_config.bootstrap_list[0].1.get_public_key(),
    );
    let temp_dir = TempDir::new().unwrap();
    let db = Arc::new(RwLock::new(Box::new(MassaDB::new(MassaDBConfig {
        path: temp_dir.path().to_path_buf(),
        max_history_length: 10,
        max_final_state_elements_size: 100_000_000,
        max_versioning_elements_size: 100_000_000,
        thread_count: THREAD_COUNT,
        max_ledger_backups: 10,
    }))
        as Box<(dyn MassaDBController + 'static)>));
    let rolls_path = PathBuf::from_str("../massa-node/base_config/initial_rolls.json").unwrap();
    let final_state_config = FinalStateConfig::default();
    let server_thread = std::thread::Builder::new()
        .name("test_binders::server_thread".to_string())
        .spawn({
            move || {
                manage_bootstrap(
                    &bootstrap_config,
                    &mut server,
                    Arc::new(RwLock::new(get_random_final_state_bootstrap(
                        PoSFinalState::new(
                            final_state_config.pos_config.clone(),
                            "",
                            &rolls_path,
                            Box::new(selector_controller.clone()),
                            db.clone(),
                        )
                        .unwrap(),
                        final_state_config,
                        db,
                    ))),
                    version(),
                    Box::new(consensus_controller),
                    Box::new(protocol_controller),
                    Instant::now().checked_add(timeout).unwrap(),
                )
            }
        })
        .unwrap();

    let client_thread = std::thread::Builder::new()
        .name("test_binders::server_thread".to_string())
        .spawn({
            move || {
                client.handshake(version()).unwrap();
                std::thread::sleep(timeout.checked_add(Duration::from_millis(100)).unwrap());
                client_clone
                    .write_all(b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA")
                    .unwrap()
            }
        })
        .unwrap();

    let res = server_thread.join().unwrap();
    match res {
        Err(BootstrapError::Interrupted(_)) => (),
        _ => panic!("The server should have been interrupted"),
    }
    client_thread.join().unwrap();
}

#[test]
fn test_staying_connected_pass_handshake_but_deadline_during_data_exchange() {
    let read_timeout_number = 2000;
    let read_timeout = Duration::from_millis(read_timeout_number);
    let (mut bootstrap_config, server_keypair): (BootstrapConfig, KeyPair) =
        BOOTSTRAP_CONFIG_KEYPAIR.clone();
    bootstrap_config.read_timeout = MassaTime::from_millis(read_timeout_number);
    let server = std::net::TcpListener::bind("localhost:0").unwrap();
    let addr = server.local_addr().unwrap();
    let client = std::net::TcpStream::connect(addr).unwrap();
    let server = server.accept().unwrap();
    let version = || Version::from_str("TEST.1.10").unwrap();
    let timeout: Duration = Duration::from_millis(800);
    let consensus_controller = MockConsensusController::new();
    let mut protocol_controller = MockProtocolController::new();
    protocol_controller
        .expect_get_bootstrap_peers()
        .returning(|| Ok(BootstrapPeers(vec![])));
    let selector_controller = MockSelectorControllerWrapper::new();

    let mut server = BootstrapServerBinder::new(
        server.0,
        server_keypair,
        BootstrapSrvBindCfg {
            rate_limit: u64::MAX,
            thread_count: THREAD_COUNT,
            max_datastore_key_length: MAX_DATASTORE_KEY_LENGTH,
            randomness_size_bytes: BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
            consensus_bootstrap_part_size: CONSENSUS_BOOTSTRAP_PART_SIZE,
            write_error_timeout: MassaTime::from_millis(1000),
        },
        None,
    );
    let mut client = BootstrapClientBinder::test_default(
        client,
        bootstrap_config.bootstrap_list[0].1.get_public_key(),
    );
    let temp_dir = TempDir::new().unwrap();
    let db = Arc::new(RwLock::new(Box::new(MassaDB::new(MassaDBConfig {
        path: temp_dir.path().to_path_buf(),
        max_history_length: 10,
        max_final_state_elements_size: 100_000_000,
        max_versioning_elements_size: 100_000_000,
        thread_count: THREAD_COUNT,
        max_ledger_backups: 10,
    }))
        as Box<(dyn MassaDBController + 'static)>));
    let rolls_path = PathBuf::from_str("../massa-node/base_config/initial_rolls.json").unwrap();
    let final_state_config = FinalStateConfig::default();
    let server_thread = std::thread::Builder::new()
        .name("test_binders::server_thread".to_string())
        .spawn({
            move || {
                manage_bootstrap(
                    &bootstrap_config,
                    &mut server,
                    Arc::new(RwLock::new(get_random_final_state_bootstrap(
                        PoSFinalState::new(
                            final_state_config.pos_config.clone(),
                            "",
                            &rolls_path,
                            Box::new(selector_controller.clone()),
                            db.clone(),
                        )
                        .unwrap(),
                        final_state_config,
                        db,
                    ))),
                    version(),
                    Box::new(consensus_controller),
                    Box::new(protocol_controller),
                    Instant::now().checked_add(timeout).unwrap(),
                )
            }
        })
        .unwrap();

    let client_thread = std::thread::Builder::new()
        .name("test_binders::server_thread".to_string())
        .spawn({
            move || {
                client.handshake(version()).unwrap();
                // Pass the bootstrap time sent by server
                client.next_timeout(Some(read_timeout)).unwrap();
                for _ in 0..10 {
                    let _ = client.send_timeout(
                        &BootstrapClientMessage::AskBootstrapPeers,
                        Some(Duration::from_millis(1000)),
                    );
                    let _ = client.next_timeout(Some(read_timeout));
                    std::thread::sleep(timeout.div_f32(5.0));
                }
            }
        })
        .unwrap();

    let res = server_thread.join().unwrap();
    // if someone doesn't send anything in a data exchange phase the server consider this exchange as finished and so finish without problems
    match res {
        Ok(()) => (),
        _ => panic!("The server should stop without problems"),
    }
    client_thread.join().unwrap();
}

// serial test for time-taken sensitive tests: reduces parallelism noise
#[test]
#[serial]
fn test_client_drip_feed() {
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
            rate_limit: u64::MAX,
            thread_count: THREAD_COUNT,
            max_datastore_key_length: MAX_DATASTORE_KEY_LENGTH,
            randomness_size_bytes: BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
            consensus_bootstrap_part_size: CONSENSUS_BOOTSTRAP_PART_SIZE,
            write_error_timeout: MassaTime::from_millis(1000),
        },
        None,
    );
    let mut client = BootstrapClientBinder::test_default(
        client,
        bootstrap_config.bootstrap_list[0].1.get_public_key(),
    );

    let start = std::time::Instant::now();
    let server_thread = std::thread::Builder::new()
        .name("test_binders::server_thread".to_string())
        .spawn({
            move || {
                server.handshake_timeout(version(), None).unwrap();

                let message = server
                    .next_timeout(Some(Duration::from_secs(1)))
                    .unwrap_err();
                match message {
                    BootstrapError::TimedOut(message) => {
                        assert_eq!(message.to_string(), "deadline has elapsed");
                        assert_eq!(message.kind(), std::io::ErrorKind::TimedOut);
                    }
                    message => panic!("expected timeout error, got {:?}", message),
                }
                std::mem::forget(server);
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
                // give a message size that we can drip-feed
                client_clone.write_all(&[0, 0, 0, 120]).unwrap();
                for i in 0..120 {
                    client_clone.write_all(&[i]).unwrap();
                    client_clone.flush().unwrap();
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        })
        .unwrap();

    server_thread.join().unwrap();
    assert!(
        start.elapsed() > Duration::from_millis(1000),
        "elapsed {:?}",
        start.elapsed()
    );
    assert!(
        start.elapsed() < Duration::from_millis(1100),
        "elapsed {:?}",
        start.elapsed()
    );
    client_thread.join().unwrap();
}

// serial test for time-taken sensitive tests: reduces parallelism noise
/// Following a handshake, the server and client will exchange messages.
///
/// We use the limiter te ensure that these message exchanges each take slightly more than 10s
#[test]
#[serial]
fn test_bandwidth() {
    let (bootstrap_config, server_keypair): &(BootstrapConfig, KeyPair) = &BOOTSTRAP_CONFIG_KEYPAIR;
    let server = std::net::TcpListener::bind("localhost:0").unwrap();
    let addr = server.local_addr().unwrap();
    let client = std::net::TcpStream::connect(addr).unwrap();
    let server = server.accept().unwrap();

    let mut server = BootstrapServerBinder::new(
        server.0,
        server_keypair.clone(),
        BootstrapSrvBindCfg {
            rate_limit: 100,
            thread_count: THREAD_COUNT,
            max_datastore_key_length: MAX_DATASTORE_KEY_LENGTH,
            randomness_size_bytes: BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
            consensus_bootstrap_part_size: CONSENSUS_BOOTSTRAP_PART_SIZE,
            write_error_timeout: MassaTime::from_millis(1000),
        },
        Some(100),
    );
    let client_cfg = BootstrapClientBinder::test_default_config();
    let mut client = BootstrapClientBinder::new(
        client,
        bootstrap_config.bootstrap_list[0].1.get_public_key(),
        client_cfg,
        Some(100),
    );
    let err_str = ['A'; 1_000].into_iter().collect::<String>();
    let srv_err_str = err_str.clone();

    let millis_limit = {
        #[cfg(target_os = "windows")]
        {
            18_000
        }

        #[cfg(target_os = "macos")]
        {
            30_500
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            11_500
        }
    };
    let server_thread = std::thread::Builder::new()
        .name("test_binders::server_thread".to_string())
        .spawn({
            move || {
                let version: Version = Version::from_str("TEST.1.10").unwrap();

                server.handshake_timeout(version, None).unwrap();

                std::thread::sleep(Duration::from_secs(1));
                let before = Instant::now();
                let message = server.next_timeout(None).unwrap();
                match message {
                    BootstrapClientMessage::BootstrapError { error } => {
                        assert_eq!(error, srv_err_str);
                    }
                    _ => panic!("Bad message receive: Expected a peers list message"),
                }
                let dur = before.elapsed();
                //assert!(dur > Duration::from_secs(9));
                assert!(dur < Duration::from_millis(millis_limit));

                std::thread::sleep(Duration::from_secs(1));
                let before = Instant::now();
                server
                    .send_timeout(
                        BootstrapServerMessage::BootstrapError { error: srv_err_str },
                        None,
                    )
                    .unwrap();
                let dur = before.elapsed();
                //assert!(dur > Duration::from_secs(9), "{dur:?}");
                assert!(dur < Duration::from_millis(millis_limit));
            }
        })
        .unwrap();

    let client_thread = std::thread::Builder::new()
        .name("test_binders::server_thread".to_string())
        .spawn({
            move || {
                let version: Version = Version::from_str("TEST.1.10").unwrap();

                client.handshake(version).unwrap();

                std::thread::sleep(Duration::from_secs(1));
                let before = Instant::now();
                client
                    .send_timeout(
                        &BootstrapClientMessage::BootstrapError {
                            error: err_str.clone(),
                        },
                        None,
                    )
                    .unwrap();
                let dur = before.elapsed();
                //assert!(dbg!(dur) > Duration::from_secs(9), "{dur:?}");
                assert!(dur < Duration::from_millis(millis_limit));

                std::thread::sleep(Duration::from_secs(1));
                let before = Instant::now();
                let message = client.next_timeout(None).unwrap();
                match message {
                    BootstrapServerMessage::BootstrapError { error } => {
                        assert_eq!(error, err_str);
                    }
                    _ => panic!("Bad message receive: Expected a peers list message"),
                }
                let dur = before.elapsed();
                //assert!(dur > Duration::from_secs(9));
                assert!(dur < Duration::from_millis(millis_limit));
            }
        })
        .unwrap();

    server_thread.join().unwrap();
    client_thread.join().unwrap();
}
