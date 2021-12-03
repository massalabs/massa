// Copyright (c) 2021 MASSA LABS <info@massa.net>

use super::super::binders::{ReadBinder, WriteBinder};
use super::mock_establisher::MockEstablisherInterface;
use super::{mock_establisher, tools};
use crate::handshake_worker::HandshakeWorker;
use crate::messages::Message;
use crate::start_network_controller;
use crate::{
    NetworkCommandSender, NetworkConfig, NetworkEvent, NetworkEventReceiver, NetworkManager,
    PeerInfo,
};
use massa_hash::hash::Hash;
use models::node::NodeId;
use models::{
    Address, Amount, BlockId, Operation, OperationContent, OperationType, SerializeCompact, Version,
};
use signature::{derive_public_key, generate_random_private_key, sign};
use std::str::FromStr;
use std::{
    future::Future,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::Path,
    time::Duration,
};
use tempfile::NamedTempFile;
use time::UTime;
use tokio::time::sleep;
use tokio::{sync::oneshot, task::JoinHandle, time::timeout};
use tracing::trace;

pub const BASE_NETWORK_CONTROLLER_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(169, 202, 0, 10));

pub fn get_dummy_block_id(s: &str) -> BlockId {
    BlockId(Hash::from(s.as_bytes()))
}

/// generate a named temporary JSON peers file
pub fn generate_peers_file(peer_vec: &Vec<PeerInfo>) -> NamedTempFile {
    use std::io::prelude::*;
    let peers_file_named = NamedTempFile::new().expect("cannot create temp file");
    serde_json::to_writer_pretty(peers_file_named.as_file(), &peer_vec)
        .expect("unable to write peers file");
    peers_file_named
        .as_file()
        .seek(std::io::SeekFrom::Start(0))
        .expect("could not seek file");
    peers_file_named
}

fn get_temp_private_key_file() -> NamedTempFile {
    NamedTempFile::new().expect("cannot create temp file")
}
/// create a NetworkConfig with typical values
pub fn create_network_config(
    network_controller_port: u16,
    peers_file_path: &Path,
) -> NetworkConfig {
    // Init the serialization context with a default,
    // can be overwritten with a more specific one in the test.
    models::init_serialization_context(models::SerializationContext {
        max_block_operations: 1024,
        parent_count: 2,
        max_peer_list_length: 128,
        max_message_size: 3 * 1024 * 1024,
        max_block_size: 3 * 1024 * 1024,
        max_bootstrap_blocks: 100,
        max_bootstrap_cliques: 100,
        max_bootstrap_deps: 100,
        max_bootstrap_children: 100,
        max_ask_blocks_per_message: 10,
        max_operations_per_message: 1024,
        max_endorsements_per_message: 1024,
        max_bootstrap_message_size: 100000000,
        max_bootstrap_pos_entries: 1000,
        max_bootstrap_pos_cycles: 5,
        max_block_endorsements: 8,
    });

    NetworkConfig {
        bind: format!("0.0.0.0:{}", network_controller_port)
            .parse()
            .unwrap(),
        routable_ip: Some(BASE_NETWORK_CONTROLLER_IP),
        protocol_port: network_controller_port,
        connect_timeout: UTime::from(3000),
        peers_file: peers_file_path.to_path_buf(),
        target_out_nonbootstrap_connections: 10,
        wakeup_interval: UTime::from(3000),
        target_bootstrap_connections: 0,
        max_out_bootstrap_connection_attempts: 1,
        max_in_nonbootstrap_connections: 100,
        max_in_connections_per_ip: 100,
        max_out_nonbootstrap_connection_attempts: 100,
        max_idle_peers: 100,
        max_banned_peers: 100,
        max_advertise_length: 10,
        peers_file_dump_interval: UTime::from(30000),
        max_message_size: 3 * 1024 * 1024,
        message_timeout: UTime::from(5000u64),
        ask_peer_list_interval: UTime::from(50000u64),
        private_key_file: get_temp_private_key_file().path().to_path_buf(),
        max_ask_blocks_per_message: 10,
        max_operations_per_message: 1024,
        max_endorsements_per_message: 1024,
        max_send_wait: UTime::from(100),
        ban_timeout: UTime::from(100_000_000),
    }
}

/// Establish a full alive connection to the controller
///
/// * establishes connection
/// * performs handshake
/// * waits for NetworkEvent::NewConnection with returned node
///
/// Returns:
/// * nodeid we just connected to
/// * binders used to communicate with that node
pub async fn full_connection_to_controller(
    network_event_receiver: &mut NetworkEventReceiver,
    mock_interface: &mut MockEstablisherInterface,
    mock_addr: SocketAddr,
    connect_timeout_ms: u64,
    event_timeout_ms: u64,
    rw_timeout_ms: u64,
) -> (NodeId, ReadBinder, WriteBinder) {
    // establish connection towards controller
    let (mock_read_half, mock_write_half) = timeout(
        Duration::from_millis(connect_timeout_ms),
        mock_interface.connect_to_controller(&mock_addr),
    )
    .await
    .expect("connection towards controller timed out")
    .expect("connection towards controller failed");

    // perform handshake
    let private_key = generate_random_private_key();
    let public_key = derive_public_key(&private_key);
    let mock_node_id = NodeId(public_key);
    let (_, read_binder, write_binder) = HandshakeWorker::new(
        mock_read_half,
        mock_write_half,
        mock_node_id,
        private_key,
        rw_timeout_ms.into(),
        Version::from_str("TEST.1.2").unwrap(),
    )
    .run()
    .await
    .expect("handshake failed");

    // wait for a NetworkEvent::NewConnection event
    wait_network_event(
        network_event_receiver,
        event_timeout_ms.into(),
        |msg| match msg {
            NetworkEvent::NewConnection(conn_node_id) => {
                if conn_node_id == mock_node_id {
                    Some(())
                } else {
                    None
                }
            }
            _ => None,
        },
    )
    .await
    .expect("did not receive NewConnection event with expected node id");

    (mock_node_id, read_binder, write_binder)
}

/// try to establish a connection to the controller and expect rejection
pub async fn rejected_connection_to_controller(
    network_event_receiver: &mut NetworkEventReceiver,
    mock_interface: &mut MockEstablisherInterface,
    mock_addr: SocketAddr,
    connect_timeout_ms: u64,
    event_timeout_ms: u64,
    rw_timeout_ms: u64,
) {
    // establish connection towards controller
    let (mock_read_half, mock_write_half) = timeout(
        Duration::from_millis(connect_timeout_ms),
        mock_interface.connect_to_controller(&mock_addr),
    )
    .await
    .expect("connection towards controller timed out")
    .expect("connection towards controller failed");

    // perform handshake and ignore errors
    let private_key = generate_random_private_key();
    let public_key = derive_public_key(&private_key);
    let mock_node_id = NodeId(public_key);
    let _handshake_res = HandshakeWorker::new(
        mock_read_half,
        mock_write_half,
        mock_node_id,
        private_key,
        rw_timeout_ms.into(),
        Version::from_str("TEST.1.2").unwrap(),
    )
    .run()
    .await;

    // wait for NetworkEvent::NewConnection or NetworkEvent::ConnectionClosed events to NOT happen
    if wait_network_event(
        network_event_receiver,
        event_timeout_ms.into(),
        |msg| match msg {
            NetworkEvent::NewConnection(conn_node_id) => {
                if conn_node_id == mock_node_id {
                    Some(())
                } else {
                    None
                }
            }
            NetworkEvent::ConnectionClosed(conn_node_id) => {
                if conn_node_id == mock_node_id {
                    Some(())
                } else {
                    None
                }
            }
            _ => None,
        },
    )
    .await
    .is_some()
    {
        panic!("unexpected node connection event detected");
    }
}

/// establish a full alive connection from the network controller
/// note: fails if the controller attempts a connection to another IP first

/// * wait for the incoming connection attempt, check address and accept
/// * perform handshake
/// * wait for a NetworkEvent::NewConnection event
///
/// Returns:
/// * nodeid we just connected to
/// * binders used to communicate with that node
pub async fn full_connection_from_controller(
    network_event_receiver: &mut NetworkEventReceiver,
    mock_interface: &mut MockEstablisherInterface,
    peer_addr: SocketAddr,
    connect_timeout_ms: u64,
    event_timeout_ms: u64,
    rw_timeout_ms: u64,
) -> (NodeId, ReadBinder, WriteBinder) {
    // wait for the incoming connection attempt, check address and accept
    let (mock_read_half, mock_write_half, ctl_addr, resp_tx) = timeout(
        Duration::from_millis(connect_timeout_ms),
        mock_interface.wait_connection_attempt_from_controller(),
    )
    .await
    .expect("timed out while waiting for connection from controller")
    .expect("failed getting connection from controller");
    assert_eq!(ctl_addr, peer_addr, "unexpected controller IP");
    resp_tx.send(true).expect("resp_tx failed");

    // perform handshake
    let private_key = generate_random_private_key();
    let public_key = derive_public_key(&private_key);
    let mock_node_id = NodeId(public_key);
    let (_controller_node_id, read_binder, write_binder) = HandshakeWorker::new(
        mock_read_half,
        mock_write_half,
        mock_node_id,
        private_key,
        rw_timeout_ms.into(),
        Version::from_str("TEST.1.2").unwrap(),
    )
    .run()
    .await
    .expect("handshake failed");

    // wait for a NetworkEvent::NewConnection event
    wait_network_event(
        network_event_receiver,
        event_timeout_ms.into(),
        |evt| match evt {
            NetworkEvent::NewConnection(node_id) => {
                if node_id == mock_node_id {
                    Some(())
                } else {
                    None
                }
            }
            _ => None,
        },
    )
    .await
    .expect("did not receive expected node connection event");

    (mock_node_id, read_binder, write_binder)
}

pub async fn wait_network_event<F, T>(
    network_event_receiver: &mut NetworkEventReceiver,
    timeout: UTime,
    filter_map: F,
) -> Option<T>
where
    F: Fn(NetworkEvent) -> Option<T>,
{
    let timer = sleep(timeout.into());
    tokio::pin!(timer);
    loop {
        tokio::select! {
            evt_opt = network_event_receiver.wait_event() => match evt_opt {
                Ok(orig_evt) => if let Some(res_evt) = filter_map(orig_evt) { return Some(res_evt); },
                _ => panic!("network event channel died")
            },
            _ = &mut timer => return None
        }
    }
}

pub async fn incoming_message_drain_start(
    read_binder: ReadBinder,
) -> (JoinHandle<ReadBinder>, oneshot::Sender<()>) {
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let join_handle = tokio::spawn(async move {
        let mut stop = stop_rx;
        let mut r_binder = read_binder;
        loop {
            tokio::select! {
                _ = &mut stop => break,
                val = r_binder.next() => match val {
                    Err(_) => break,
                    Ok(None) => break,
                    Ok(Some(_)) => {} // ignore
                }
            }
        }
        trace!("incoming_message_drain_start end drain message");
        r_binder
    });
    (join_handle, stop_tx)
}

pub async fn advertise_peers_in_connection(write_binder: &mut WriteBinder, peer_list: Vec<IpAddr>) {
    write_binder
        .send(&Message::PeerList(peer_list))
        .await
        .expect("could not send peer list");
}

pub async fn incoming_message_drain_stop(
    handle: (JoinHandle<ReadBinder>, oneshot::Sender<()>),
) -> ReadBinder {
    let (join_handle, stop_tx) = handle;
    let _ = stop_tx.send(()); // ignore failure which just means that the drain has quit on socket drop
    join_handle.await.expect("could not join message drain")
}

pub fn get_transaction(expire_period: u64, fee: u64) -> (Operation, u8) {
    let sender_priv = generate_random_private_key();
    let sender_pub = derive_public_key(&sender_priv);

    let recv_priv = generate_random_private_key();
    let recv_pub = derive_public_key(&recv_priv);

    let op = OperationType::Transaction {
        recipient_address: Address::from_public_key(&recv_pub).unwrap(),
        amount: Amount::default(),
    };
    let content = OperationContent {
        fee: Amount::from_str(&fee.to_string()).unwrap(),
        op,
        sender_public_key: sender_pub,
        expire_period,
    };
    let hash = Hash::from(&content.to_bytes_compact().unwrap());
    let signature = sign(&hash, &sender_priv).unwrap();

    (
        Operation { content, signature },
        Address::from_public_key(&sender_pub).unwrap().get_thread(2),
    )
}

/// Runs a consensus test, passing a mock pool controller to it.
pub async fn network_test<F, V>(cfg: NetworkConfig, temp_peers_file: NamedTempFile, test: F)
where
    F: FnOnce(
        NetworkCommandSender,
        NetworkEventReceiver,
        NetworkManager,
        MockEstablisherInterface,
    ) -> V,
    V: Future<
        Output = (
            NetworkEventReceiver,
            NetworkManager,
            MockEstablisherInterface,
            Vec<(JoinHandle<ReadBinder>, oneshot::Sender<()>)>,
        ),
    >,
{
    // create establisher
    let (establisher, mock_interface) = mock_establisher::new();

    // launch network controller
    let (network_event_sender, network_event_receiver, network_manager, _private_key, _node_id) =
        start_network_controller(
            cfg.clone(),
            establisher,
            0,
            None,
            Version::from_str("TEST.1.2").unwrap(),
        )
        .await
        .expect("could not start network controller");

    // Call test func.
    // force _mock_interface return to avoid to be dropped before the end of the test (network_manager.stop).
    let (network_event_receiver, network_manager, _mock_interface, conn_to_drain_list) = test(
        network_event_sender,
        network_event_receiver,
        network_manager,
        mock_interface,
    )
    .await;

    network_manager
        .stop(network_event_receiver)
        .await
        .expect("error while stopping network");

    for conn_drain in conn_to_drain_list {
        tools::incoming_message_drain_stop(conn_drain).await;
    }

    temp_peers_file.close().unwrap();
}
