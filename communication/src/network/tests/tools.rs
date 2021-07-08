use super::super::binders::{ReadBinder, WriteBinder};
use super::mock_establisher::MockEstablisherInterface;
use crate::common::NodeId;
use crate::network::handshake_worker::HandshakeWorker;
use crate::network::messages::Message;
use crate::network::{NetworkConfig, NetworkEvent, NetworkEventReceiver, PeerInfo};
use crypto::signature::SignatureEngine;
use models::SerializationContext;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::Path,
    time::Duration,
};
use tempfile::NamedTempFile;
use time::UTime;
use tokio::{sync::oneshot, task::JoinHandle, time::timeout};

use tokio::time::sleep;

pub const BASE_NETWORK_CONTROLLER_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(169, 202, 0, 10));

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
) -> (NetworkConfig, SerializationContext) {
    (
        NetworkConfig {
            bind: format!("0.0.0.0:{}", network_controller_port)
                .parse()
                .unwrap(),
            routable_ip: Some(BASE_NETWORK_CONTROLLER_IP),
            protocol_port: network_controller_port,
            connect_timeout: UTime::from(3000),
            peers_file: peers_file_path.to_path_buf(),
            target_out_connections: 10,
            wakeup_interval: UTime::from(3000),
            max_in_connections: 100,
            max_in_connections_per_ip: 100,
            max_out_connnection_attempts: 100,
            max_idle_peers: 100,
            max_banned_peers: 100,
            max_advertise_length: 10,
            peers_file_dump_interval: UTime::from(30000),
            max_message_size: 3 * 1024 * 1024,
            message_timeout: UTime::from(5000u64),
            ask_peer_list_interval: UTime::from(50000u64),
            private_key_file: get_temp_private_key_file().path().to_path_buf(),
            max_ask_blocks_per_message: 10,
        },
        SerializationContext {
            max_block_size: 1024 * 1024,
            max_block_operations: 1024,
            parent_count: 2,
            max_peer_list_length: 128,
            max_message_size: 3 * 1024 * 1024,
            max_bootstrap_blocks: 100,
            max_bootstrap_cliques: 100,
            max_bootstrap_deps: 100,
            max_bootstrap_children: 100,
            max_ask_blocks_per_message: 10,
        },
    )
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
    serialization_context: SerializationContext,
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
    let sig_engine = SignatureEngine::new();
    let private_key = SignatureEngine::generate_random_private_key();
    let public_key = sig_engine.derive_public_key(&private_key);
    let mock_node_id = NodeId(public_key);
    let (_, read_binder, write_binder) = HandshakeWorker::new(
        serialization_context,
        mock_read_half,
        mock_write_half,
        mock_node_id,
        private_key,
        rw_timeout_ms.into(),
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
    serialization_context: SerializationContext,
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
    let sig_engine = SignatureEngine::new();
    let private_key = SignatureEngine::generate_random_private_key();
    let public_key = sig_engine.derive_public_key(&private_key);
    let mock_node_id = NodeId(public_key);
    let _handshake_res = HandshakeWorker::new(
        serialization_context,
        mock_read_half,
        mock_write_half,
        mock_node_id,
        private_key,
        rw_timeout_ms.into(),
    )
    .run()
    .await;

    // wait for NetworkEvent::NewConnection or NetworkEvent::ConnectionClosed events to NOT happen
    if let Some(_) =
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
    serialization_context: SerializationContext,
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
    let sig_engine = SignatureEngine::new();
    let private_key = SignatureEngine::generate_random_private_key();
    let public_key = sig_engine.derive_public_key(&private_key);
    let mock_node_id = NodeId(public_key);
    let (_controller_node_id, read_binder, write_binder) = HandshakeWorker::new(
        serialization_context,
        mock_read_half,
        mock_write_half,
        mock_node_id,
        private_key,
        rw_timeout_ms.into(),
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
