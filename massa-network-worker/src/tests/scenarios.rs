// Copyright (c) 2022 MASSA LABS <info@massa.net>

// To start alone RUST_BACKTRACE=1 cargo test -- --nocapture --test-threads=1
use super::tools;
use crate::messages::{Message, MessageDeserializer};
use crate::node_worker::NodeWorker;
use crate::tests::tools::{get_dummy_block_id, get_transaction};
use crate::NetworkError;
use crate::NetworkEvent;
use crate::{
    binders::{ReadBinder, WriteBinder},
    NetworkConfig,
};
use enum_map::enum_map;
use enum_map::EnumMap;
use massa_hash::Hash;
use massa_models::config::{
    ENDORSEMENT_COUNT, MAX_ADVERTISE_LENGTH, MAX_ASK_BLOCKS_PER_MESSAGE,
    MAX_DATASTORE_VALUE_LENGTH, MAX_DENUNCIATIONS_PER_BLOCK_HEADER, MAX_ENDORSEMENTS_PER_MESSAGE,
    MAX_FUNCTION_NAME_LENGTH, MAX_MESSAGE_SIZE, MAX_OPERATIONS_PER_BLOCK,
    MAX_OPERATIONS_PER_MESSAGE, MAX_OPERATION_DATASTORE_ENTRY_COUNT,
    MAX_OPERATION_DATASTORE_KEY_LENGTH, MAX_OPERATION_DATASTORE_VALUE_LENGTH, MAX_PARAMETERS_SIZE,
    THREAD_COUNT,
};
use massa_models::{
    block_id::BlockId,
    endorsement::{Endorsement, EndorsementSerializer},
    node::NodeId,
    secure_share::SecureShareContent,
    slot::Slot,
};
use massa_network_exports::{settings::PeerTypeConnectionConfig, NodeCommand, NodeEvent};
use massa_network_exports::{
    AskForBlocksInfo, BlockInfoReply, ConnectionClosureReason, ConnectionId, HandshakeErrorType,
    PeerInfo, PeerType,
};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use serial_test::serial;
use std::collections::HashMap;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::trace;

fn default_testing_peer_type_enum_map() -> EnumMap<PeerType, PeerTypeConnectionConfig> {
    enum_map! {
        PeerType::Bootstrap => PeerTypeConnectionConfig {
            target_out_connections: 1,
            max_out_attempts: 1,
            max_in_connections: 1,
        },
        PeerType::WhiteListed => PeerTypeConnectionConfig {
            target_out_connections: 2,
            max_out_attempts: 2,
            max_in_connections: 3,
        },
        PeerType::Standard => PeerTypeConnectionConfig {
            target_out_connections: 0,
            max_out_attempts: 0,
            max_in_connections: 2,
        }
    }
}

/// Test that a node worker can shutdown even if the event channel is full,
/// and that sending additional node commands during shutdown does not deadlock.
#[tokio::test]
#[serial]
async fn test_node_worker_shutdown() {
    let bind_port: u16 = 50_000;
    let temp_peers_file = super::tools::generate_peers_file(&[]);
    let network_conf = NetworkConfig::scenarios_default(bind_port, temp_peers_file.path());
    let (duplex_controller, _duplex_mock) = tokio::io::duplex(1);
    let (duplex_mock_read, duplex_mock_write) = tokio::io::split(duplex_controller);
    let reader = ReadBinder::new(
        duplex_mock_read,
        f64::INFINITY,
        MAX_MESSAGE_SIZE,
        MessageDeserializer::new(
            THREAD_COUNT,
            ENDORSEMENT_COUNT,
            MAX_ADVERTISE_LENGTH,
            MAX_ASK_BLOCKS_PER_MESSAGE,
            MAX_OPERATIONS_PER_BLOCK,
            MAX_OPERATIONS_PER_MESSAGE,
            MAX_ENDORSEMENTS_PER_MESSAGE,
            MAX_DATASTORE_VALUE_LENGTH,
            MAX_FUNCTION_NAME_LENGTH,
            MAX_PARAMETERS_SIZE,
            MAX_OPERATION_DATASTORE_ENTRY_COUNT,
            MAX_OPERATION_DATASTORE_KEY_LENGTH,
            MAX_OPERATION_DATASTORE_VALUE_LENGTH,
            MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            Some(0),
        ),
    );
    let writer = WriteBinder::new(duplex_mock_write, f64::INFINITY, MAX_MESSAGE_SIZE);

    // Note: both channels have size 1.
    let (node_command_tx, node_command_rx) = mpsc::channel::<NodeCommand>(1);
    let (node_event_tx, _node_event_rx) = mpsc::channel::<NodeEvent>(1);

    let keypair = KeyPair::generate(0).unwrap();
    let mock_node_id = NodeId::new(keypair.get_public_key());

    let node_worker_command_tx = node_command_tx.clone();
    let node_fn_handle = tokio::spawn(async move {
        NodeWorker::new(
            network_conf,
            mock_node_id,
            reader,
            writer,
            node_worker_command_tx,
            node_command_rx,
            node_event_tx,
        )
        .run_loop()
        .await
    });

    // Shutdown the worker.
    node_command_tx
        .send(NodeCommand::Close(ConnectionClosureReason::Normal))
        .await
        .unwrap();

    // Send a bunch of additional commands until the channel is closed,
    // which would deadlock if not properly handled by the worker.
    loop {
        if node_command_tx
            .send(NodeCommand::Close(ConnectionClosureReason::Normal))
            .await
            .is_err()
        {
            break;
        }
    }

    node_fn_handle.await.unwrap().unwrap();
}

/// Test that a node worker can send an operations message.
#[tokio::test]
#[serial]
async fn test_node_worker_operations_message() {
    let bind_port: u16 = 50_000;
    let temp_peers_file = super::tools::generate_peers_file(&[]);
    let network_conf = NetworkConfig::scenarios_default(bind_port, temp_peers_file.path());
    let (duplex_controller, _duplex_mock) = tokio::io::duplex(1);
    let (duplex_mock_read, duplex_mock_write) = tokio::io::split(duplex_controller);
    let reader = ReadBinder::new(
        duplex_mock_read,
        f64::INFINITY,
        MAX_MESSAGE_SIZE,
        MessageDeserializer::new(
            THREAD_COUNT,
            ENDORSEMENT_COUNT,
            MAX_ADVERTISE_LENGTH,
            MAX_ASK_BLOCKS_PER_MESSAGE,
            MAX_OPERATIONS_PER_BLOCK,
            MAX_OPERATIONS_PER_MESSAGE,
            MAX_ENDORSEMENTS_PER_MESSAGE,
            MAX_DATASTORE_VALUE_LENGTH,
            MAX_FUNCTION_NAME_LENGTH,
            MAX_PARAMETERS_SIZE,
            MAX_OPERATION_DATASTORE_ENTRY_COUNT,
            MAX_OPERATION_DATASTORE_KEY_LENGTH,
            MAX_OPERATION_DATASTORE_VALUE_LENGTH,
            MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            Some(0),
        ),
    );
    let writer = WriteBinder::new(duplex_mock_write, f64::INFINITY, MAX_MESSAGE_SIZE);

    // Note: both channels have size 1.
    let (node_command_tx, node_command_rx) = mpsc::channel::<NodeCommand>(1);
    let (node_event_tx, _node_event_rx) = mpsc::channel::<NodeEvent>(1);

    let keypair = KeyPair::generate(0).unwrap();
    let mock_node_id = NodeId::new(keypair.get_public_key());

    // Create transaction.
    let transaction = get_transaction(50, 10);

    let node_worker_command_tx = node_command_tx.clone();
    let node_fn_handle = tokio::spawn(async move {
        NodeWorker::new(
            network_conf,
            mock_node_id,
            reader,
            writer,
            node_worker_command_tx,
            node_command_rx,
            node_event_tx,
        )
        .run_loop()
        .await
    });

    // Send operations message.
    node_command_tx
        .send(NodeCommand::SendOperations(
            vec![transaction].into_iter().collect(),
        ))
        .await
        .unwrap();

    // TODO: add some infra to receive the message via the duplex mock, and assert it is what is expected.

    // Send a bunch of additional commands until the channel is closed,
    // which would deadlock if not properly handled by the worker.
    loop {
        if node_command_tx
            .send(NodeCommand::Close(ConnectionClosureReason::Normal))
            .await
            .is_err()
        {
            break;
        }
    }

    node_fn_handle.await.unwrap().unwrap();
}

// test connecting two different peers simultaneously to the controller
// then attempt to connect to controller from an already connected peer to test max_in_connections_per_ip
// then try to connect a third peer to test max_in_connection
#[tokio::test]
#[serial]
async fn test_multiple_connections_to_controller() {
    // setup logging
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/

    // test config
    let bind_port: u16 = 50_000;
    let temp_peers_file = super::tools::generate_peers_file(&[]);
    let network_conf = NetworkConfig {
        peer_types_config: default_testing_peer_type_enum_map(),
        max_in_connections_per_ip: 1,
        ..NetworkConfig::scenarios_default(bind_port, temp_peers_file.path())
    };

    let mock1_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 202, 0, 11)), bind_port);
    let mock2_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 202, 0, 12)), bind_port);
    let mock3_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 202, 0, 13)), bind_port);

    tools::network_test(
        network_conf.clone(),
        temp_peers_file,
        async move |_network_command_sender,
                    mut network_event_receiver,
                    network_manager,
                    mut mock_interface| {
            // note: the peers list is empty so the controller will not attempt outgoing connections

            // 1) connect peer1 to controller
            let (conn1_id, conn1_r, _conn1_w) = tools::full_connection_to_controller(
                &mut network_event_receiver,
                &mut mock_interface,
                mock1_addr,
                1_000u64,
                1_000u64,
                1_000u64,
                ConnectionId(0),
            )
            .await;
            let conn1_drain = tools::incoming_message_drain_start(conn1_r).await; // drained l110

            // 2) connect peer2 to controller
            let (conn2_id, conn2_r, _conn2_w) = tools::full_connection_to_controller(
                &mut network_event_receiver,
                &mut mock_interface,
                mock2_addr,
                1_000u64,
                1_000u64,
                1_000u64,
                ConnectionId(3),
            )
            .await;
            assert_ne!(
                conn1_id, conn2_id,
                "IDs of simultaneous connections should be different"
            );
            let conn2_drain = tools::incoming_message_drain_start(conn2_r).await; // drained l109

            // 3) try to establish an extra connection from peer1 to controller with max_in_connections_per_ip = 1
            let err: NetworkError = tools::rejected_connection_to_controller(
                &mut network_event_receiver,
                &mut mock_interface,
                mock1_addr,
                1_000u64,
                1_000u64,
                1_000u64,
                ConnectionId(2),
            )
            .await;

            if !matches!(
                err,
                NetworkError::HandshakeError(HandshakeErrorType::PeerListReceived(_))
            ) {
                panic!(
                    "We were supposed to handle a peer list here\nReceived {}",
                    err
                )
            }

            // 4) try to establish an third connection to controller with max_in_connections = 2
            let _: NetworkError = tools::rejected_connection_to_controller(
                &mut network_event_receiver,
                &mut mock_interface,
                mock3_addr,
                1_000u64,
                1_000u64,
                1_000u64,
                ConnectionId(3),
            )
            .await;
            (
                network_event_receiver,
                network_manager,
                mock_interface,
                vec![conn1_drain, conn2_drain],
            )
        },
    )
    .await;
}

// test peer ban
// add an advertised peer
// accept controller's connection atttempt to that peer
// establish a connection from that peer to controller
// signal closure + ban of one of the connections
// expect an event to signal the banning of the other one
// close the other one
// attempt to connect banned peer to controller : must fail
// make sure there are no connection attempts incoming from peer
#[tokio::test]
#[serial]
async fn test_peer_ban() {
    /*//setup logging
    stderrlog::new()
        .verbosity(4)
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()
        .unwrap();*/

    // test config
    let bind_port: u16 = 50_000;

    let mock_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 202, 0, 11)), bind_port);
    // add advertised peer to controller
    let temp_peers_file = super::tools::generate_peers_file(&[PeerInfo::new(mock_addr.ip(), true)]);

    let network_conf = NetworkConfig {
        wakeup_interval: 1000.into(),
        ..NetworkConfig::scenarios_default(bind_port, temp_peers_file.path())
    };

    tools::network_test(
        network_conf.clone(),
        temp_peers_file,
        async move |network_command_sender,
                    mut network_event_receiver,
                    network_manager,
                    mut mock_interface| {
            // accept connection from controller to peer
            let (conn1_id, conn1_r, conn1_w) = tools::full_connection_from_controller(
                &mut network_event_receiver,
                &mut mock_interface,
                mock_addr,
                1_000u64,
                1_000u64,
                1_000u64,
                ConnectionId(0),
            )
            .await;
            let conn1_drain = tools::incoming_message_drain_start(conn1_r).await;

            trace!("test_peer_ban first connection done");

            // connect peer to controller
            let (_conn2_id, conn2_r, conn2_w) = tools::full_connection_to_controller(
                &mut network_event_receiver,
                &mut mock_interface,
                mock_addr,
                1_000u64,
                1_000u64,
                1_000u64,
                ConnectionId(1),
            )
            .await;
            let conn2_drain = tools::incoming_message_drain_start(conn2_r).await;
            trace!("test_peer_ban second connection done");

            // ban connection1.
            network_command_sender
                .node_ban_by_ids(vec![conn1_id])
                .await
                .expect("error during send ban command.");

            // make sure the ban message was processed
            sleep(Duration::from_millis(200)).await;

            // stop conn1 and conn2
            tools::incoming_message_drain_stop(conn1_drain).await;
            drop(conn1_w);
            tools::incoming_message_drain_stop(conn2_drain).await;
            drop(conn2_w);
            sleep(Duration::from_millis(200)).await;

            // drain all messages
            let _ = tools::wait_network_event(&mut network_event_receiver, 500.into(), |_msg| {
                Option::<()>::None
            })
            .await;

            // attempt a new connection from peer to controller: should be rejected
            let _: NetworkError = tools::rejected_connection_to_controller(
                &mut network_event_receiver,
                &mut mock_interface,
                mock_addr,
                1_000u64,
                1_000u64,
                1_000u64,
                ConnectionId(2),
            )
            .await;

            // unban connection1.
            network_command_sender
                .node_unban_ips(vec![mock_addr.ip()])
                .await
                .expect("error during send unban command.");

            // wait for new connection attempt
            sleep(network_conf.wakeup_interval.to_duration()).await;

            // accept connection from controller to peer after unban
            let (_conn1_id, conn1_r, _conn1_w) = tools::full_connection_from_controller(
                &mut network_event_receiver,
                &mut mock_interface,
                mock_addr,
                1_000u64,
                1_000u64,
                1_000u64,
                ConnectionId(3),
            )
            .await;
            let conn1_drain_bis = tools::incoming_message_drain_start(conn1_r).await;

            trace!("test_peer_ban unbanned connection done");
            (
                network_event_receiver,
                network_manager,
                mock_interface,
                vec![conn1_drain_bis],
            )
        },
    )
    .await;
}

// test peer ban by ip
// add an advertised peer
// accept controller's connection atttempt to that peer
// establish a connection from that peer to controller
// signal closure + ban of one of the connections
// expect an event to signal the banning of the other one
// close the other one
// attempt to connect banned peer to controller : must fail
// make sure there are no connection attempts incoming from peer
#[tokio::test]
#[serial]
async fn test_peer_ban_by_ip() {
    /*//setup logging
    stderrlog::new()
        .verbosity(4)
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()
        .unwrap();*/

    // test config
    let bind_port: u16 = 50_000;

    let mock_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 202, 0, 11)), bind_port);
    // add advertised peer to controller
    let temp_peers_file = super::tools::generate_peers_file(&[PeerInfo::new(mock_addr.ip(), true)]);

    let network_conf = NetworkConfig {
        wakeup_interval: 1000.into(),
        ..NetworkConfig::scenarios_default(bind_port, temp_peers_file.path())
    };

    tools::network_test(
        network_conf.clone(),
        temp_peers_file,
        async move |network_command_sender,
                    mut network_event_receiver,
                    network_manager,
                    mut mock_interface| {
            // accept connection from controller to peer
            let (_, conn1_r, conn1_w) = tools::full_connection_from_controller(
                &mut network_event_receiver,
                &mut mock_interface,
                mock_addr,
                1_000u64,
                1_000u64,
                1_000u64,
                ConnectionId(0),
            )
            .await;
            let conn1_drain = tools::incoming_message_drain_start(conn1_r).await;

            trace!("test_peer_ban first connection done");

            // connect peer to controller
            let (_conn2_id, conn2_r, conn2_w) = tools::full_connection_to_controller(
                &mut network_event_receiver,
                &mut mock_interface,
                mock_addr,
                1_000u64,
                1_000u64,
                1_000u64,
                ConnectionId(1),
            )
            .await;
            let conn2_drain = tools::incoming_message_drain_start(conn2_r).await;
            trace!("test_peer_ban second connection done");

            // ban connection1.
            network_command_sender
                .node_ban_by_ips(vec![mock_addr.ip()])
                .await
                .expect("error during send ban command.");

            // make sure the ban message was processed
            sleep(Duration::from_millis(200)).await;

            // stop conn1 and conn2
            tools::incoming_message_drain_stop(conn1_drain).await;
            drop(conn1_w);
            tools::incoming_message_drain_stop(conn2_drain).await;
            drop(conn2_w);
            sleep(Duration::from_millis(200)).await;

            // drain all messages
            let _ = tools::wait_network_event(&mut network_event_receiver, 500.into(), |_msg| {
                Option::<()>::None
            })
            .await;

            // attempt a new connection from peer to controller: should be rejected
            let _: NetworkError = tools::rejected_connection_to_controller(
                &mut network_event_receiver,
                &mut mock_interface,
                mock_addr,
                1_000u64,
                1_000u64,
                1_000u64,
                ConnectionId(2),
            )
            .await;

            // unban connection1.
            network_command_sender
                .node_unban_ips(vec![mock_addr.ip()])
                .await
                .expect("error during send unban command.");

            // wait for new connection attempt
            sleep(network_conf.wakeup_interval.to_duration()).await;

            // accept connection from controller to peer after unban
            let (_conn1_id, conn1_r, _conn1_w) = tools::full_connection_from_controller(
                &mut network_event_receiver,
                &mut mock_interface,
                mock_addr,
                1_000u64,
                1_000u64,
                1_000u64,
                ConnectionId(3),
            )
            .await;
            let conn1_drain_bis = tools::incoming_message_drain_start(conn1_r).await;

            trace!("test_peer_ban unbanned connection done");
            (
                network_event_receiver,
                network_manager,
                mock_interface,
                vec![conn1_drain_bis],
            )
        },
    )
    .await;
}

// test merge_advertised_peer_list, advertised and wakeup_interval:
//   setup one non-advertised peer
//   use merge_advertised_peer_list to add another peer (this one is advertised)
//   start by refusing a connection attempt from controller
//   (making sure it is aimed at the advertised peer)
//   then check the time it takes the controller to try a new connection to advertised peer
//   (accept the connection)
//   then check that there are no further connection attempts at all
#[tokio::test]
#[serial]
async fn test_advertised_and_wakeup_interval() {
    // setup logging
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/

    // test config
    let bind_port: u16 = 50_000;
    let mock_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 202, 0, 12)), bind_port);
    let mock_ignore_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 202, 0, 13)), bind_port);
    let temp_peers_file = super::tools::generate_peers_file(&[PeerInfo {
        ip: mock_ignore_addr.ip(),
        peer_type: PeerType::Bootstrap,
        last_alive: None,
        last_failure: None,
        advertised: false,
        active_out_connection_attempts: 0,
        active_out_connections: 0,
        active_in_connections: 0,
        banned: false,
    }]);
    let network_conf = NetworkConfig {
        wakeup_interval: MassaTime::from_millis(500),
        connect_timeout: MassaTime::from_millis(2000),
        ..NetworkConfig::scenarios_default(bind_port, temp_peers_file.path())
    };

    tools::network_test(
        network_conf.clone(),
        temp_peers_file,
        async move |_network_command_sender,
                    mut network_event_receiver,
                    network_manager,
                    mut mock_interface| {
            // 1) open a connection, advertize peer, disconnect
            {
                let (conn2_id, conn2_r, mut conn2_w) = tools::full_connection_to_controller(
                    &mut network_event_receiver,
                    &mut mock_interface,
                    mock_addr,
                    1_000u64,
                    1_000u64,
                    1_000u64,
                    ConnectionId(0),
                )
                .await;
                tools::advertise_peers_in_connection(&mut conn2_w, vec![mock_addr.ip()]).await;
                // drop the connection
                drop(conn2_r);
                drop(conn2_w);
                // wait for the message signalling the closure of the connection
                tools::wait_network_event(
                    &mut network_event_receiver,
                    1000.into(),
                    |msg| match msg {
                        NetworkEvent::ConnectionClosed(closed_node_id) => {
                            if closed_node_id == conn2_id {
                                Some(())
                            } else {
                                None
                            }
                        }
                        _ => None,
                    },
                )
                .await
                .expect("connection closure message not received");
            };

            // 2) refuse the first connection attempt coming from controller towards advertised peer
            {
                let (_, _, addr, accept_tx) = tokio::time::timeout(
                    Duration::from_millis(1500),
                    mock_interface.wait_connection_attempt_from_controller(),
                )
                .await
                .expect("wait_connection_attempt_from_controller timed out")
                .expect("wait_connection_attempt_from_controller failed");
                assert_eq!(addr, mock_addr, "unexpected connection attempt address");
                accept_tx.send(false).expect("accept_tx failed");
            }

            // 3) Next connection attempt from controller, accept connection
            let (_conn_id, _conn1_w, conn1_drain) = {
                // drained l357
                let start_instant = Instant::now();
                let (conn_id, conn1_r, conn1_w) = tools::full_connection_from_controller(
                    &mut network_event_receiver,
                    &mut mock_interface,
                    mock_addr,
                    (network_conf.wakeup_interval.to_millis() as u128 * 3u128)
                        .try_into()
                        .unwrap(),
                    1_000u64,
                    1_000u64,
                    ConnectionId(1),
                )
                .await;
                if start_instant.elapsed() < network_conf.wakeup_interval.to_duration() {
                    panic!("controller tried to reconnect after a too short delay");
                }
                let drain_h = tools::incoming_message_drain_start(conn1_r).await; // drained l357
                (conn_id, conn1_w, drain_h)
            };

            // 4) check that there are no further connection attempts from controller
            if tools::wait_network_event(
                &mut network_event_receiver,
                1000.into(),
                |msg| match msg {
                    NetworkEvent::NewConnection(_) => Some(()),
                    _ => None,
                },
            )
            .await
            .is_some()
            {
                panic!("a connection event was emitted by controller while none were expected");
            }
            (
                network_event_receiver,
                network_manager,
                mock_interface,
                vec![conn1_drain],
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_block_not_found() {
    // setup logging
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/

    // test config
    let bind_port: u16 = 50_000;

    let mock_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 202, 0, 11)), bind_port);
    // add advertised peer to controller
    let temp_peers_file = super::tools::generate_peers_file(&[PeerInfo {
        ip: mock_addr.ip(),
        peer_type: PeerType::Bootstrap,
        last_alive: None,
        last_failure: None,
        advertised: true,
        active_out_connection_attempts: 0,
        active_out_connections: 0,
        active_in_connections: 0,
        banned: false,
    }]);
    let network_conf = NetworkConfig {
        peer_types_config: default_testing_peer_type_enum_map(),
        max_ask_blocks: 3,
        ..NetworkConfig::scenarios_default(bind_port, temp_peers_file.path())
    };

    tools::network_test(
        network_conf.clone(),
        temp_peers_file,
        async move |network_command_sender,
                    mut network_event_receiver,
                    network_manager,
                    mut mock_interface| {
            // accept connection from controller to peer
            let (conn1_id, mut conn1_r, mut conn1_w) = tools::full_connection_from_controller(
                &mut network_event_receiver,
                &mut mock_interface,
                mock_addr,
                1_000u64,
                1_000u64,
                1_000u64,
                ConnectionId(0),
            )
            .await;
            // let conn1_drain= tools::incoming_message_drain_start(conn1_r).await;

            // Send ask for block message from connected peer
            let wanted_hash = get_dummy_block_id("default_val");
            conn1_w.send(&Message::AskForBlocks(vec![(wanted_hash, AskForBlocksInfo::Info)])).await.unwrap();

            // assert it is sent to protocol
            if let Some((mut list, node)) =
                tools::wait_network_event(&mut network_event_receiver, 1000.into(), |msg| match msg
                {
                    NetworkEvent::AskedForBlocks { list, node } => Some((list, node)),
                    _ => None,
                })
                .await
            {
                assert_eq!(list.pop().unwrap().0, wanted_hash);
                assert_eq!(node, conn1_id);
            } else {
                panic!("Timeout while waiting for asked for block event");
            }

            // reply with block not found
            network_command_sender
                .send_block_info(conn1_id, vec![(wanted_hash,  BlockInfoReply::NotFound)])
                .await
                .unwrap();

            // let mut  conn1_r = conn1_drain.0.await.unwrap();
            // assert that block not found is sent to node

            let timer = sleep(Duration::from_millis(500));
            tokio::pin!(timer);
            loop {
                tokio::select! {
                    evt = conn1_r.next() => {
                        let evt = evt.unwrap().unwrap().1;
                        if let Message::ReplyForBlocks(mut info) = evt {
                            let info = info.pop().unwrap();
                            assert_eq!(info.0, wanted_hash);
                            if let BlockInfoReply::NotFound = info.1 {
                                break;
                            }
                        }
                    },
                    _ = &mut timer => panic!("timeout reached waiting for message")
                }
            }

            // test send AskForBlocks with more max_ask_blocks_per_message using node_worker split in several message function.
            let mut block_list: HashMap<NodeId, Vec<(BlockId, AskForBlocksInfo)>> = HashMap::new();
            let hash_list = vec![
                (get_dummy_block_id("default_val1"), AskForBlocksInfo::Info),
                (get_dummy_block_id("default_val2"), AskForBlocksInfo::Info),
                (get_dummy_block_id("default_val3"), AskForBlocksInfo::Info),
                (get_dummy_block_id("default_val4"), AskForBlocksInfo::Info),
            ];
            block_list.insert(conn1_id, hash_list);

            network_command_sender
                .ask_for_block_list(block_list)
                .await
                .unwrap();
            // receive 2 list
            let timer = sleep(Duration::from_millis(100));
            tokio::pin!(timer);
            loop {
                tokio::select! {
                    evt = conn1_r.next() => {
                        let evt = evt.unwrap().unwrap().1;
                        if let Message::AskForBlocks(list1) = evt {
                            let list1: Vec<BlockId> = list1.iter().cloned().map(|(id, _)| id).collect();
                            assert!(list1.contains(&get_dummy_block_id("default_val1")));
                            assert!(list1.contains(&get_dummy_block_id("default_val2")));
                            assert!(list1.contains(&get_dummy_block_id("default_val3")));
                            break;
                        }},
                    _ = &mut timer => panic!("timeout reached waiting for message")
                }
            }
            let timer = sleep(Duration::from_millis(100));
            tokio::pin!(timer);
            loop {
                tokio::select! {
                    evt = conn1_r.next() => {
                        let evt = evt.unwrap().unwrap().1;
                        if let Message::AskForBlocks(list2) = evt {
                            assert!(list2.iter().cloned().map(|(id, _)| id).any(|x| x == get_dummy_block_id("default_val4")));
                            break;
                        }
                    },
                    _ = &mut timer => panic!("timeout reached waiting for message")
                }
            }

            // test with max_ask_blocks_per_message > 3 sending the message straight to the connection.
            // the message is not rejected by the receiver.
            let wanted_hash1 = (get_dummy_block_id("default_val1"), AskForBlocksInfo::Info);
            let wanted_hash2 = (get_dummy_block_id("default_val2"), AskForBlocksInfo::Info);
            let wanted_hash3 = (get_dummy_block_id("default_val3"), AskForBlocksInfo::Info);
            let wanted_hash4 = (get_dummy_block_id("default_val4"), AskForBlocksInfo::Info);
            conn1_w.send(&Message::AskForBlocks(vec![
                        wanted_hash1,
                        wanted_hash2,
                        wanted_hash3,
                        wanted_hash4,
                    ])).await.unwrap();
            // assert it is sent to protocol
            if tools::wait_network_event(
                &mut network_event_receiver,
                1000.into(),
                |msg| match msg {
                    NetworkEvent::AskedForBlocks { list, node } => Some((list, node)),
                    _ => None,
                },
            )
            .await
            .is_none()
            {
                panic!("AskedForBlocks with more max_ask_blocks_per_message forward blocks");
            }
            let conn1_drain = tools::incoming_message_drain_start(conn1_r).await;
            (
                network_event_receiver,
                network_manager,
                mock_interface,
                vec![conn1_drain],
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_retry_connection_closed() {
    // setup logging
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/

    // test config
    let bind_port: u16 = 50_000;

    let mock_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 202, 0, 11)), bind_port);
    // add advertised peer to controller
    let temp_peers_file = super::tools::generate_peers_file(&[PeerInfo {
        ip: mock_addr.ip(),
        peer_type: PeerType::Bootstrap,
        last_alive: None,
        last_failure: None,
        advertised: true,
        active_out_connection_attempts: 0,
        active_out_connections: 0,
        active_in_connections: 0,
        banned: false,
    }]);
    let network_conf = NetworkConfig {
        peer_types_config: default_testing_peer_type_enum_map(),
        ..NetworkConfig::scenarios_default(bind_port, temp_peers_file.path())
    };

    tools::network_test(
        network_conf.clone(),
        temp_peers_file,
        async move |network_command_sender,
                    mut network_event_receiver,
                    network_manager,
                    mut mock_interface| {
            let (node_id, _read, _write) = tools::full_connection_to_controller(
                &mut network_event_receiver,
                &mut mock_interface,
                mock_addr,
                1_000u64,
                1_000u64,
                1_000u64,
                ConnectionId(0),
            )
            .await;

            // Ban the node.
            network_command_sender
                .node_ban_by_ids(vec![node_id])
                .await
                .expect("error during send ban command.");

            // Make sure network sends a dis-connect event.
            if let Some(node) =
                tools::wait_network_event(&mut network_event_receiver, 1000.into(), |msg| match msg
                {
                    NetworkEvent::ConnectionClosed(node) => Some(node),
                    _ => None,
                })
                .await
            {
                assert_eq!(node, node_id);
            } else {
                panic!("Timeout while waiting for connection closed event");
            }

            // Send a command for a node not found in active.
            network_command_sender
                .send_block_info(node_id, vec![])
                .await
                .unwrap();

            // Make sure network re-sends a dis-connect event.
            if let Some(node) =
                tools::wait_network_event(&mut network_event_receiver, 1000.into(), |msg| match msg
                {
                    NetworkEvent::ConnectionClosed(node) => Some(node),
                    _ => None,
                })
                .await
            {
                assert_eq!(node, node_id);
            } else {
                panic!("Timeout while waiting for connection closed event");
            }
            (
                network_event_receiver,
                network_manager,
                mock_interface,
                vec![],
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_operation_messages() {
    // setup logging
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/

    // test config
    let bind_port: u16 = 50_000;

    let mock_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 202, 0, 11)), bind_port);
    // add advertised peer to controller
    let temp_peers_file = super::tools::generate_peers_file(&[PeerInfo {
        ip: mock_addr.ip(),
        peer_type: PeerType::Bootstrap,
        last_alive: None,
        last_failure: None,
        advertised: true,
        active_out_connection_attempts: 0,
        active_out_connections: 0,
        active_in_connections: 0,
        banned: false,
    }]);
    let network_conf = NetworkConfig {
        peer_types_config: default_testing_peer_type_enum_map(),
        max_ask_blocks: 3,
        ..NetworkConfig::scenarios_default(bind_port, temp_peers_file.path())
    };

    tools::network_test(
        network_conf.clone(),
        temp_peers_file,
        async move |network_command_sender,
                    mut network_event_receiver,
                    network_manager,
                    mut mock_interface| {
            // accept connection from controller to peer
            let (conn1_id, mut conn1_r, mut conn1_w) = tools::full_connection_from_controller(
                &mut network_event_receiver,
                &mut mock_interface,
                mock_addr,
                1_000u64,
                1_000u64,
                1_000u64,
                ConnectionId(0),
            )
            .await;
            // let conn1_drain= tools::incoming_message_drain_start(conn1_r).await;

            // Send transaction message from connected peer
            let transaction = get_transaction(50, 10);
            conn1_w
                .send(&Message::Operations(vec![transaction.clone()]))
                .await
                .unwrap();

            // assert it is sent to protocol
            if let Some((operations, node)) =
                tools::wait_network_event(&mut network_event_receiver, 1000.into(), |msg| match msg
                {
                    NetworkEvent::ReceivedOperations { operations, node } => {
                        Some((operations, node))
                    }
                    _ => None,
                })
                .await
            {
                assert_eq!(operations.len(), 1);
                assert_eq!(node, conn1_id);
            } else {
                panic!("Timeout while waiting for received operations event");
            }

            let transaction2 = get_transaction(10, 50);
            let ref_id2 = transaction2.id;

            // reply with another transaction
            network_command_sender
                .send_operations(conn1_id, vec![transaction2].into_iter().collect())
                .await
                .unwrap();

            // let mut  conn1_r = conn1_drain.0.await.unwrap();
            // assert that transaction is sent to node

            let timer = sleep(Duration::from_millis(500));
            tokio::pin!(timer);
            loop {
                tokio::select! {
                    evt = conn1_r.next() => {
                        let evt = evt.unwrap().unwrap().1;
                        if let Message::Operations(op) = evt {
                            assert_eq!(op.len(), 1);
                            assert_eq!(op[0].id, ref_id2);
                            break;
                        }
                    },
                    _ = &mut timer => panic!("timeout reached waiting for message")
                }
            }
            let conn1_drain = tools::incoming_message_drain_start(conn1_r).await;
            (
                network_event_receiver,
                network_manager,
                mock_interface,
                vec![conn1_drain],
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_endorsements_messages() {
    // setup logging
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/

    // test config
    let bind_port: u16 = 50_000;

    let mock_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 202, 0, 11)), bind_port);
    // add advertised peer to controller
    let temp_peers_file = super::tools::generate_peers_file(&[PeerInfo {
        ip: mock_addr.ip(),
        peer_type: PeerType::Bootstrap,
        last_alive: None,
        last_failure: None,
        advertised: true,
        active_out_connection_attempts: 0,
        active_out_connections: 0,
        active_in_connections: 0,
        banned: false,
    }]);
    let network_conf = NetworkConfig {
        peer_types_config: default_testing_peer_type_enum_map(),
        max_ask_blocks: 3,
        ..NetworkConfig::scenarios_default(bind_port, temp_peers_file.path())
    };

    tools::network_test(
        network_conf.clone(),
        temp_peers_file,
        async move |network_command_sender,
                    mut network_event_receiver,
                    network_manager,
                    mut mock_interface| {
            // accept connection from controller to peer
            let (conn1_id, mut conn1_r, mut conn1_w) = tools::full_connection_from_controller(
                &mut network_event_receiver,
                &mut mock_interface,
                mock_addr,
                1_000u64,
                1_000u64,
                1_000u64,
                ConnectionId(0),
            )
            .await;
            // let conn1_drain= tools::incoming_message_drain_start(conn1_r).await;

            let sender_keypair = KeyPair::generate(0).unwrap();

            let content = Endorsement {
                slot: Slot::new(10, 1),
                index: 0,
                endorsed_block: BlockId(Hash::compute_from(&[])),
            };
            let endorsement = Endorsement::new_verifiable(
                content.clone(),
                EndorsementSerializer::new(),
                &sender_keypair,
            )
            .unwrap();
            conn1_w
                .send(&Message::Endorsements(vec![endorsement.clone()]))
                .await
                .unwrap();

            // assert it is sent to protocol
            if let Some((endorsements, node)) =
                tools::wait_network_event(&mut network_event_receiver, 1000.into(), |msg| match msg
                {
                    NetworkEvent::ReceivedEndorsements { endorsements, node } => {
                        Some((endorsements, node))
                    }
                    _ => None,
                })
                .await
            {
                assert_eq!(endorsements.len(), 1);
                assert_eq!(endorsement, endorsements[0]);
                assert_eq!(node, conn1_id);
            } else {
                panic!("Timeout while waiting for endorsement event.");
            }

            let sender_keypair = KeyPair::generate(0).unwrap();

            let content = Endorsement {
                slot: Slot::new(11, 1),
                index: 0,
                endorsed_block: BlockId(Hash::compute_from(&[])),
            };
            let endorsement = Endorsement::new_verifiable(
                content.clone(),
                EndorsementSerializer::new(),
                &sender_keypair,
            )
            .unwrap();
            // let ref_id = endorsement.id;

            // reply with another endorsement
            network_command_sender
                .send_endorsements(conn1_id, vec![endorsement.clone()])
                .await
                .unwrap();

            let timer = sleep(Duration::from_millis(500));
            tokio::pin!(timer);
            loop {
                tokio::select! {
                    evt = conn1_r.next() => {
                        let evt = evt.unwrap().unwrap().1;
                        if let Message::Endorsements(endorsements) = evt {
                            assert_eq!(endorsements.len(), 1);
                            assert_eq!(endorsement, endorsements[0]);
                            break;
                        }
                    },
                    _ = &mut timer => panic!("timeout reached waiting for message")
                }
            }
            let conn1_drain = tools::incoming_message_drain_start(conn1_r).await;
            (
                network_event_receiver,
                network_manager,
                mock_interface,
                vec![conn1_drain],
            )
        },
    )
    .await;
}
