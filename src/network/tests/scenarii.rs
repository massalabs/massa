//to start alone RUST_BACKTRACE=1 cargo test scenarii -- --nocapture --test-threads=1

use super::super::default_network_controller::DefaultNetworkController;
use super::super::network_controller::{ConnectionClosureReason, NetworkController, NetworkEvent};
use super::super::peer_info_database::PeerInfo;
use super::mock_establisher;
use super::tools;
use std::convert::TryInto;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::time::{sleep, timeout};

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
    .unwrap(); */

    //test config
    let bind_port: u16 = 50_000;
    let temp_peers_file = super::tools::generate_peers_file(&vec![]);
    let mut network_conf = super::tools::create_network_config(bind_port, &temp_peers_file.path());
    network_conf.max_in_connections = 2;
    network_conf.max_in_connections_per_ip = 1;

    let mock1_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 202, 0, 11)), bind_port);
    let mock2_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 202, 0, 12)), bind_port);
    let mock3_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 202, 0, 13)), bind_port);

    // init network
    let (establisher, mut mock_interface) = mock_establisher::new();
    let mut network = DefaultNetworkController::new(&network_conf, establisher)
        .await
        .unwrap();

    // note: the peers list is empty so the controller will not attempt outgoing connections

    // 1) connect peer1 to controller
    let conn1_id = tools::full_connection_to_controller(
        &mut network,
        &mut mock_interface,
        mock1_addr,
        1_000u64,
        1_000u64,
        1_000u64,
    )
    .await;

    // 2) connect peer2 to controller
    let conn2_id = tools::full_connection_to_controller(
        &mut network,
        &mut mock_interface,
        mock2_addr,
        1_000u64,
        1_000u64,
        1_000u64,
    )
    .await;
    assert_ne!(
        conn1_id, conn2_id,
        "IDs of simultaneous connections should be different"
    );

    // 3) try to establish an extra connection from peet1 to controller
    {
        let _ = mock_interface
            .connect_to_controller(&mock1_addr)
            .await
            .expect("connection to controller failed");
        // no event should occur / have occurred
        if let Ok(_) = timeout(Duration::from_millis(1000), network.wait_event()).await {
            panic!("an event was emitted by controller while none were expected");
        }
    }

    // 4) try to establish an third connection to controller
    {
        let _ = mock_interface
            .connect_to_controller(&mock3_addr)
            .await
            .expect("connection to controller failed");
        // no event should occur / have occurred
        if let Ok(_) = timeout(Duration::from_millis(1000), network.wait_event()).await {
            panic!("an event was emitted by controller while none were expected");
        }
    }

    // 5) close connections and stop
    network
        .connection_closed(conn1_id, ConnectionClosureReason::Normal)
        .await;
    network
        .connection_closed(conn2_id, ConnectionClosureReason::Normal)
        .await;
    network.stop().await;
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
    .unwrap(); */

    //test config
    let bind_port: u16 = 50_000;
    let temp_peers_file = super::tools::generate_peers_file(&vec![]);
    let mut network_conf = super::tools::create_network_config(bind_port, &temp_peers_file.path());
    network_conf.target_out_connections = 10;

    let mock_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 202, 0, 11)), bind_port);

    // init network
    let (establisher, mut mock_interface) = mock_establisher::new();
    let mut network = DefaultNetworkController::new(&network_conf, establisher)
        .await
        .unwrap();

    // add advertised peer to controller
    network
        .merge_advertised_peer_list(vec![mock_addr.ip()])
        .await;

    // accept connection from controller to peer
    let conn1_id = tools::full_connection_from_controller(
        &mut network,
        &mut mock_interface,
        mock_addr,
        1_000u64,
        1_000u64,
        1_000u64,
    )
    .await;

    // connect to peeer to controller
    let conn2_id = tools::full_connection_to_controller(
        &mut network,
        &mut mock_interface,
        mock_addr,
        1_000u64,
        1_000u64,
        1_000u64,
    )
    .await;

    // signal the controller that conn1 is closed + banned
    network
        .connection_closed(conn1_id, ConnectionClosureReason::Banned)
        .await;

    // wait for controller to signal that conn2 is banned as well. Close conn2.
    if let Ok(NetworkEvent::ConnectionBanned(ban_id)) =
        timeout(Duration::from_millis(1000), network.wait_event()).await
    {
        assert_eq!(
            ban_id, conn2_id,
            "wrong connection ban signalled by controller"
        );
    } else {
        panic!("no event or unexpected event emitted by controller");
    }
    network
        .connection_closed(conn2_id, ConnectionClosureReason::Normal)
        .await;

    // attempt a new connection from peer to controller
    let _ = mock_interface
        .connect_to_controller(&mock_addr)
        .await
        .expect("connection to controller failed");

    // no event should occur / have occurred (no incoming or outgoing connections succeeeded)
    if let Ok(_) = timeout(Duration::from_millis(1000), network.wait_event()).await {
        panic!("an event was emitted by controller while none were expected");
    }

    // close
    network.stop().await;
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
    .unwrap(); */

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
    let mut network_conf = super::tools::create_network_config(bind_port, &temp_peers_file.path());
    network_conf.wakeup_interval = Duration::from_millis(2000);
    network_conf.connect_timeout = Duration::from_millis(1000);

    // init network controller
    let (establisher, mut mock_interface) = mock_establisher::new();
    let mut network = DefaultNetworkController::new(&network_conf, establisher)
        .await
        .unwrap();

    // 1) advertise a peer and wait a bit for connection attempts to start
    network
        .merge_advertised_peer_list(vec![mock_addr.ip()])
        .await;
    sleep(Duration::from_millis(500)).await;

    // 2) refuse the first connection attempt coming from controller towards advertised peer
    {
        let (_, _, addr, accept_tx) = mock_interface
            .wait_connection_attempt_from_controller()
            .await
            .expect("wait_connection_attempt_from_controller failed");
        assert_eq!(addr, mock_addr, "unexpected connection attempt address");
        accept_tx.send(false).expect("accept_tx failed");
    }

    // 3) time the next connection attempt from controller, accept connection
    let conn_id = {
        let start_instant = Instant::now();
        let conn_id = tools::full_connection_from_controller(
            &mut network,
            &mut mock_interface,
            mock_addr,
            (network_conf.wakeup_interval.as_millis() * 3u128)
                .try_into()
                .unwrap(),
            1_000u64,
            1_000u64,
        )
        .await;
        if start_instant.elapsed() < network_conf.wakeup_interval {
            panic!("controller tried to reconnect after a too short delay");
        }
        conn_id
    };

    // 4) check that there are no further connection attempts from controller
    {
        // no event should occur / have occurred
        if let Ok(_) = timeout(Duration::from_millis(1_000u64), network.wait_event()).await {
            panic!("an event was emitted by controller while none were expected");
        }
    }

    // 5) close connection
    network
        .connection_closed(conn_id, ConnectionClosureReason::Normal)
        .await;
    network.stop().await;
    temp_peers_file.close().unwrap();
}
