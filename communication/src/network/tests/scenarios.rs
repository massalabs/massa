//to start alone RUST_BACKTRACE=1 cargo test -- --nocapture --test-threads=1
use super::{mock_establisher, tools};
use crate::network::messages::Message;
use crate::network::NetworkEvent;
use crate::network::{start_network_controller, PeerInfo};
use crypto::hash::Hash;
use std::{
    convert::TryInto,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::{Duration, Instant},
};
use time::UTime;
use tokio::time::sleep;

// test connecting two different peers simultaneously to the controller
// then attempt to connect to controller from an already connected peer to test max_in_connections_per_ip
// then try to connect a third peer to test max_in_connection
#[tokio::test]
async fn test_multiple_connections_to_controller() {
    // setup logging
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/

    //test config
    let bind_port: u16 = 50_000;
    let temp_peers_file = super::tools::generate_peers_file(&vec![]);
    let (mut network_conf, serialization_context) =
        super::tools::create_network_config(bind_port, &temp_peers_file.path());
    network_conf.max_in_connections = 2;
    network_conf.max_in_connections_per_ip = 1;

    let mock1_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 202, 0, 11)), bind_port);
    let mock2_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 202, 0, 12)), bind_port);
    let mock3_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 202, 0, 13)), bind_port);

    // create establisher
    let (establisher, mut mock_interface) = mock_establisher::new();

    // launch network controller
    let (_, mut network_event_receiver, network_manager) = start_network_controller(
        network_conf.clone(),
        serialization_context.clone(),
        establisher,
    )
    .await
    .expect("could not start network controller");

    // note: the peers list is empty so the controller will not attempt outgoing connections

    // 1) connect peer1 to controller
    let (conn1_id, conn1_r, _conn1_w) = tools::full_connection_to_controller(
        &mut network_event_receiver,
        &mut mock_interface,
        mock1_addr,
        1_000u64,
        1_000u64,
        1_000u64,
        serialization_context.clone(),
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
        serialization_context.clone(),
    )
    .await;
    assert_ne!(
        conn1_id, conn2_id,
        "IDs of simultaneous connections should be different"
    );
    let conn2_drain = tools::incoming_message_drain_start(conn2_r).await; // drained l109

    // 3) try to establish an extra connection from peer1 to controller with max_in_connections_per_ip = 1
    tools::rejected_connection_to_controller(
        &mut network_event_receiver,
        &mut mock_interface,
        mock1_addr,
        1_000u64,
        1_000u64,
        1_000u64,
        serialization_context.clone(),
    )
    .await;

    // 4) try to establish an third connection to controller with max_in_connections = 2
    tools::rejected_connection_to_controller(
        &mut network_event_receiver,
        &mut mock_interface,
        mock3_addr,
        1_000u64,
        1_000u64,
        1_000u64,
        serialization_context.clone(),
    )
    .await;

    network_manager
        .stop(network_event_receiver)
        .await
        .expect("error while stopping network");

    tools::incoming_message_drain_stop(conn2_drain).await;
    tools::incoming_message_drain_stop(conn1_drain).await;

    temp_peers_file.close().unwrap();
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
async fn test_peer_ban() {
    // setup logging
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/

    //test config
    let bind_port: u16 = 50_000;

    let mock_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 202, 0, 11)), bind_port);
    //add advertised peer to controller
    let temp_peers_file = super::tools::generate_peers_file(&vec![PeerInfo {
        ip: mock_addr.ip(),
        banned: false,
        bootstrap: true,
        last_alive: None,
        last_failure: None,
        advertised: true,
        active_out_connection_attempts: 0,
        active_out_connections: 0,
        active_in_connections: 0,
    }]);

    let (mut network_conf, serialization_context) =
        super::tools::create_network_config(bind_port, &temp_peers_file.path());
    network_conf.target_out_connections = 10;

    // create establisher
    let (establisher, mut mock_interface) = mock_establisher::new();

    // launch network controller
    let (mut network_command_sender, mut network_event_receiver, network_manager) =
        start_network_controller(
            network_conf.clone(),
            serialization_context.clone(),
            establisher,
        )
        .await
        .expect("could not start network controller");

    // accept connection from controller to peer
    let (conn1_id, conn1_r, _conn1_w) = tools::full_connection_from_controller(
        &mut network_event_receiver,
        &mut mock_interface,
        mock_addr,
        1_000u64,
        1_000u64,
        1_000u64,
        serialization_context.clone(),
    )
    .await;
    let conn1_drain = tools::incoming_message_drain_start(conn1_r).await; // drained l220

    trace!("test_peer_ban first connection done");

    // connect peer to controller
    let (_conn2_id, conn2_r, _conn2_w) = tools::full_connection_to_controller(
        &mut network_event_receiver,
        &mut mock_interface,
        mock_addr,
        1_000u64,
        1_000u64,
        1_000u64,
        serialization_context.clone(),
    )
    .await;
    let conn2_drain = tools::incoming_message_drain_start(conn2_r).await; // drained l221
    trace!("test_peer_ban second connection done");

    //ban connection1.
    network_command_sender
        .ban(conn1_id)
        .await
        .expect("error during send ban command.");

    // attempt a new connection from peer to controller: should be rejected
    tools::rejected_connection_to_controller(
        &mut network_event_receiver,
        &mut mock_interface,
        mock_addr,
        1_000u64,
        1_000u64,
        1_000u64,
        serialization_context.clone(),
    )
    .await;

    // close
    network_manager
        .stop(network_event_receiver)
        .await
        .expect("error while stopping network");

    tools::incoming_message_drain_stop(conn1_drain).await;
    tools::incoming_message_drain_stop(conn2_drain).await;

    temp_peers_file.close().unwrap();
}

// test merge_advertised_peer_list, advertised and wakeup_interval:
//   setup one non-advertised peer
//   use merge_advertised_peer_list to add another peer (this one is advertised)
//   start by refusing a connection attempt from controller
//   (making sure it is aimed at the advertised peer)
//   then check the time it takes the controller to try a new connection to advertised peer
//   (accept the connection)
//   then check that thare are no further connection attempts at all
#[tokio::test]
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
    let temp_peers_file = super::tools::generate_peers_file(&vec![PeerInfo {
        ip: mock_ignore_addr.ip(),
        banned: false,
        bootstrap: true,
        last_alive: None,
        last_failure: None,
        advertised: false,
        active_out_connection_attempts: 0,
        active_out_connections: 0,
        active_in_connections: 0,
    }]);
    let (mut network_conf, serialization_context) =
        super::tools::create_network_config(bind_port, &temp_peers_file.path());
    network_conf.wakeup_interval = UTime::from(2000);
    network_conf.connect_timeout = UTime::from(1000);

    // create establisher
    let (establisher, mut mock_interface) = mock_establisher::new();

    // launch network controller
    let (_, mut network_event_receiver, network_manager) = start_network_controller(
        network_conf.clone(),
        serialization_context.clone(),
        establisher,
    )
    .await
    .expect("could not start network controller");

    // 1) open a connection, advertize peer, disconnect
    {
        let (conn2_id, conn2_r, mut conn2_w) = tools::full_connection_to_controller(
            &mut network_event_receiver,
            &mut mock_interface,
            mock_addr,
            1_000u64,
            1_000u64,
            1_000u64,
            serialization_context.clone(),
        )
        .await;
        tools::advertise_peers_in_connection(&mut conn2_w, vec![mock_addr.ip()]).await;
        // drop the connection
        drop(conn2_r);
        drop(conn2_w);
        // wait for the message signalling the closure of the connection
        tools::wait_network_event(&mut network_event_receiver, 1000.into(), |msg| match msg {
            NetworkEvent::ConnectionClosed(closed_node_id) => {
                if closed_node_id == conn2_id {
                    Some(())
                } else {
                    None
                }
            }
            _ => None,
        })
        .await
        .expect("connection closure message not received");
    };

    // 2) refuse the first connection attempt coming from controller towards advertised peer
    {
        let (_, _, addr, accept_tx) = tokio::time::timeout(
            Duration::from_millis(1000),
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
            serialization_context.clone(),
        )
        .await;
        if start_instant.elapsed() < network_conf.wakeup_interval.to_duration() {
            panic!("controller tried to reconnect after a too short delay");
        }
        let drain_h = tools::incoming_message_drain_start(conn1_r).await; // drained l357
        (conn_id, conn1_w, drain_h)
    };

    // 4) check that there are no further connection attempts from controller
    if let Some(_) =
        tools::wait_network_event(&mut network_event_receiver, 1000.into(), |msg| match msg {
            NetworkEvent::NewConnection(_) => Some(()),
            _ => None,
        })
        .await
    {
        panic!("a connection event was emitted by controller while none were expected");
    }

    network_manager
        .stop(network_event_receiver)
        .await
        .expect("error while closing");

    tools::incoming_message_drain_stop(conn1_drain).await;

    temp_peers_file.close().unwrap();
}

#[tokio::test]
async fn test_block_not_found() {
    // setup logging
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/

    //test config
    let bind_port: u16 = 50_000;

    let mock_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 202, 0, 11)), bind_port);
    //add advertised peer to controller
    let temp_peers_file = super::tools::generate_peers_file(&vec![PeerInfo {
        ip: mock_addr.ip(),
        banned: false,
        bootstrap: true,
        last_alive: None,
        last_failure: None,
        advertised: true,
        active_out_connection_attempts: 0,
        active_out_connections: 0,
        active_in_connections: 0,
    }]);

    let (mut network_conf, serialization_context) =
        super::tools::create_network_config(bind_port, &temp_peers_file.path());
    network_conf.target_out_connections = 10;

    // create establisher
    let (establisher, mut mock_interface) = mock_establisher::new();

    // launch network controller
    let (mut network_command_sender, mut network_event_receiver, network_manager) =
        start_network_controller(
            network_conf.clone(),
            serialization_context.clone(),
            establisher,
        )
        .await
        .expect("could not start network controller");

    // accept connection from controller to peer
    let (conn1_id, mut conn1_r, mut conn1_w) = tools::full_connection_from_controller(
        &mut network_event_receiver,
        &mut mock_interface,
        mock_addr,
        1_000u64,
        1_000u64,
        1_000u64,
        serialization_context.clone(),
    )
    .await;
    //let conn1_drain= tools::incoming_message_drain_start(conn1_r).await;

    // Send ask for block message from connected peer
    let wanted_hash = Hash::hash("default_val".as_bytes());
    conn1_w
        .send(&Message::AskForBlock(wanted_hash))
        .await
        .unwrap();

    // assert it is sent to protocol
    if let Some((hash, node)) =
        tools::wait_network_event(&mut network_event_receiver, 1000.into(), |msg| match msg {
            NetworkEvent::AskedForBlock { hash, node } => Some((hash, node)),
            _ => None,
        })
        .await
    {
        assert_eq!(hash, wanted_hash);
        assert_eq!(node, conn1_id);
    } else {
        panic!("Timeout while waiting for asked for block event");
    }

    // reply with block not found
    network_command_sender
        .block_not_found(conn1_id, wanted_hash)
        .await
        .unwrap();

    //let mut  conn1_r = conn1_drain.0.await.unwrap();
    // assert that block not found is sent to node

    let timer = sleep(Duration::from_millis(100));
    tokio::pin!(timer);
    loop {
        tokio::select! {
            evt = conn1_r.next() => {
                let evt = evt.unwrap().unwrap().1;
                match evt {
                Message::BlockNotFound(hash) => {assert_eq!(hash, wanted_hash); break;}
                _ => {}
            }},
            _ = &mut timer => panic!("timeout reached waiting for message")
        }
    }

    let conn1_drain = tools::incoming_message_drain_start(conn1_r).await;
    network_manager
        .stop(network_event_receiver)
        .await
        .expect("error while closing");

    tools::incoming_message_drain_stop(conn1_drain).await;

    temp_peers_file.close().unwrap();
}
