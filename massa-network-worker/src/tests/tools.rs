// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::super::binders::{ReadBinder, WriteBinder};
use super::tools;
use crate::handshake_worker::HandshakeWorker;
use crate::messages::Message;
use crate::start_network_controller;
use crate::NetworkError;
use crate::NetworkEvent;
use crate::NetworkSettings;

use massa_hash::Hash;
use massa_models::node::NodeId;
use massa_models::signed::Signed;
use massa_models::{
    Address, Amount, BlockId, Operation, OperationType, SerializeCompact, SignedOperation, Version,
};
use massa_network_exports::test_exports::mock_establisher::{self, MockEstablisherInterface};
use massa_network_exports::{
    ConnectionId, NetworkCommandSender, NetworkEventReceiver, NetworkManager, PeerInfo,
};
use massa_signature::{derive_public_key, generate_random_private_key};
use massa_storage::Storage;
use massa_time::MassaTime;
use std::str::FromStr;
use std::{
    future::Future,
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use tempfile::NamedTempFile;
use tokio::time::sleep;
use tokio::{sync::oneshot, task::JoinHandle, time::timeout};
use tracing::trace;

pub fn get_dummy_block_id(s: &str) -> BlockId {
    BlockId(Hash::compute_from(s.as_bytes()))
}

/// generate a named temporary JSON peers file
pub fn generate_peers_file(peer_vec: &[PeerInfo]) -> NamedTempFile {
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

#[cfg(test)]
/// Establish a full alive connection to the controller
///
/// * establishes connection
/// * performs handshake
/// * waits for `NetworkEvent::NewConnection` with returned node
///
/// Returns:
/// * `NodeId` we just connected to
/// * binders used to communicate with that node
pub async fn full_connection_to_controller(
    network_event_receiver: &mut NetworkEventReceiver,
    mock_interface: &mut MockEstablisherInterface,
    mock_addr: SocketAddr,
    connect_timeout_ms: u64,
    event_timeout_ms: u64,
    rw_timeout_ms: u64,
    connection_id: ConnectionId,
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
    let res = HandshakeWorker::spawn(
        mock_read_half,
        mock_write_half,
        mock_node_id,
        private_key,
        rw_timeout_ms.into(),
        Version::from_str("TEST.1.2").unwrap(),
        connection_id,
    )
    .await
    .expect("handshake creation failed")
    .1
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
    (mock_node_id, res.1, res.2)
}

/// try to establish a connection to the controller and expect rejection.
/// Return the `NetworkError` that spawned from the `HandshakeWorker`.
pub async fn rejected_connection_to_controller(
    network_event_receiver: &mut NetworkEventReceiver,
    mock_interface: &mut MockEstablisherInterface,
    mock_addr: SocketAddr,
    connect_timeout_ms: u64,
    event_timeout_ms: u64,
    rw_timeout_ms: u64,
    connection_id: ConnectionId,
) -> NetworkError {
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

    let result = HandshakeWorker::spawn(
        mock_read_half,
        mock_write_half,
        mock_node_id,
        private_key,
        rw_timeout_ms.into(),
        Version::from_str("TEST.1.2").unwrap(),
        connection_id,
    )
    .await
    .expect("handshake creation failed")
    .1;

    let ret = if let Err(err) = result {
        err
    } else {
        panic!("Handshake Operation was supposed to failed")
    };

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

    ret
}

/// Establish a full alive connection from the network controller
/// note: fails if the controller attempts a connection to another IP first

/// * wait for the incoming connection attempt, check address and accept
/// * perform handshake
/// * wait for a `NetworkEvent::NewConnection` event
///
/// Returns:
/// * `NodeId` we just connected to
/// * binders used to communicate with that node
pub async fn full_connection_from_controller(
    network_event_receiver: &mut NetworkEventReceiver,
    mock_interface: &mut MockEstablisherInterface,
    peer_addr: SocketAddr,
    connect_timeout_ms: u64,
    event_timeout_ms: u64,
    rw_timeout_ms: u64,
    connection_id: ConnectionId,
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
    let res = HandshakeWorker::spawn(
        mock_read_half,
        mock_write_half,
        mock_node_id,
        private_key,
        rw_timeout_ms.into(),
        Version::from_str("TEST.1.2").unwrap(),
        connection_id,
    )
    .await
    .expect("handshake creation failed")
    .1
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

    (mock_node_id, res.1, res.2)
}

pub async fn wait_network_event<F, T>(
    network_event_receiver: &mut NetworkEventReceiver,
    timeout: MassaTime,
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
        .send(
            &Message::PeerList(peer_list)
                .to_bytes_compact()
                .expect("Fail to serialize message"),
        )
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

pub fn get_transaction(expire_period: u64, fee: u64) -> (SignedOperation, u8) {
    let sender_priv = generate_random_private_key();
    let sender_pub = derive_public_key(&sender_priv);

    let recv_priv = generate_random_private_key();
    let recv_pub = derive_public_key(&recv_priv);

    let op = OperationType::Transaction {
        recipient_address: Address::from_public_key(&recv_pub),
        amount: Amount::default(),
    };
    let content = Operation {
        fee: Amount::from_str(&fee.to_string()).unwrap(),
        op,
        sender_public_key: sender_pub,
        expire_period,
    };

    (
        Signed::new_signed(content, &sender_priv).unwrap().1,
        Address::from_public_key(&sender_pub).get_thread(2),
    )
}

/// Runs a consensus test, passing a mock pool controller to it.
pub async fn network_test<F, V>(
    network_settings: NetworkSettings,
    temp_peers_file: NamedTempFile,
    test: F,
) where
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
    let storage: Storage = Default::default();
    // launch network controller
    let (network_event_sender, network_event_receiver, network_manager, _private_key, _node_id) =
        start_network_controller(
            network_settings,
            establisher,
            0,
            None,
            storage,
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
