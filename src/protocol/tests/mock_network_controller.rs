use crate::network::establisher::{Connector, Establisher, Listener};
use crate::network::network_controller::{
    ConnectionClosureReason, ConnectionId, NetworkController, NetworkEvent,
};
use async_trait::async_trait;
use std::io;
use std::net::{IpAddr, SocketAddr};
use tokio::io::DuplexStream;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Duration;

pub type ReadHalf = tokio::io::ReadHalf<DuplexStream>;
pub type WriteHalf = tokio::io::WriteHalf<DuplexStream>;

#[derive(Debug)]
pub struct MockListener;

#[async_trait]
impl Listener<ReadHalf, WriteHalf> for MockListener {
    async fn accept(&mut self) -> std::io::Result<(ReadHalf, WriteHalf, SocketAddr)> {
        unreachable!();
    }
}

#[derive(Debug)]
pub struct MockConnector;

#[async_trait]
impl Connector<ReadHalf, WriteHalf> for MockConnector {
    async fn connect(&mut self, _addr: SocketAddr) -> std::io::Result<(ReadHalf, WriteHalf)> {
        unreachable!();
    }
}

#[derive(Debug)]
pub struct MockEstablisher;

#[async_trait]
impl Establisher for MockEstablisher {
    type ReaderT = ReadHalf;
    type WriterT = WriteHalf;
    type ListenerT = MockListener;
    type ConnectorT = MockConnector;

    async fn get_listener(&mut self, _addr: SocketAddr) -> io::Result<Self::ListenerT> {
        unreachable!();
    }

    async fn get_connector(
        &mut self,
        _timeout_duration: Duration,
    ) -> std::io::Result<Self::ConnectorT> {
        unreachable!();
    }
}

const MAX_DUPLEX_BUFFER_SIZE: usize = 10000;

#[derive(Debug)]
pub enum MockNetworkCommand {
    MergeAdvertisedPeerList(Vec<IpAddr>),
    GetAdvertisablePeerList(oneshot::Sender<Vec<IpAddr>>),
    ConnectionClosed((ConnectionId, ConnectionClosureReason)),
    ConnectionAlive(ConnectionId),
}

pub fn new() -> (MockNetworkController, MockNetworkControllerInterface) {
    let (network_event_tx, network_event_rx) =
        mpsc::channel::<NetworkEvent<ReadHalf, WriteHalf>>(1024);
    let (network_command_tx, network_command_rx) = mpsc::channel::<MockNetworkCommand>(1024);
    (
        MockNetworkController {
            network_event_rx,
            network_command_tx,
        },
        MockNetworkControllerInterface {
            network_event_tx,
            network_command_rx,
            cur_connection_id: ConnectionId::default(),
        },
    )
}

#[derive(Debug)]
pub struct MockNetworkController {
    network_event_rx: mpsc::Receiver<NetworkEvent<ReadHalf, WriteHalf>>,
    network_command_tx: mpsc::Sender<MockNetworkCommand>,
}

/*impl<EstablisherT: Establisher + 'static> MockNetworkController<EstablisherT> {
    /// Starts a new NetworkController from NetworkConfig
    /// can panic if :
    /// - config routable_ip IP is not routable
    pub async fn new(mut establisher: EstablisherT) -> BoxResult<Self> {
        Ok(MockNetworkController { establisher })
    }
} */

#[async_trait]
impl NetworkController for MockNetworkController {
    type EstablisherT = MockEstablisher;
    type ReaderT = ReadHalf;
    type WriterT = WriteHalf;

    async fn stop(mut self) {}

    async fn wait_event(&mut self) -> NetworkEvent<ReadHalf, WriteHalf> {
        self.network_event_rx
            .recv()
            .await
            .expect("MockNetworkController wait_event channel close")
    }

    async fn merge_advertised_peer_list(&mut self, ips: Vec<IpAddr>) {
        self.network_command_tx
            .send(MockNetworkCommand::MergeAdvertisedPeerList(ips))
            .await
            .expect("network controller disappeared");
    }

    async fn get_advertisable_peer_list(&mut self) -> Vec<IpAddr> {
        let (response_tx, response_rx) = oneshot::channel::<Vec<IpAddr>>();
        self.network_command_tx
            .send(MockNetworkCommand::GetAdvertisablePeerList(response_tx))
            .await
            .expect("network controller disappeared");
        response_rx.await.expect("network controller disappeared")
    }

    async fn connection_closed(&mut self, id: ConnectionId, reason: ConnectionClosureReason) {
        self.network_command_tx
            .send(MockNetworkCommand::ConnectionClosed((id, reason)))
            .await
            .expect("network controller disappeared");
    }

    async fn connection_alive(&mut self, id: ConnectionId) {
        self.network_command_tx
            .send(MockNetworkCommand::ConnectionAlive(id))
            .await
            .expect("network controller disappeared");
    }
}

pub struct MockNetworkControllerInterface {
    network_event_tx: mpsc::Sender<NetworkEvent<ReadHalf, WriteHalf>>,
    network_command_rx: mpsc::Receiver<MockNetworkCommand>,
    cur_connection_id: ConnectionId,
}

impl MockNetworkControllerInterface {
    pub async fn wait_command(&mut self) -> Option<MockNetworkCommand> {
        Some(self.network_command_rx.recv().await?)
    }

    pub async fn new_connection(&mut self) -> (ReadHalf, WriteHalf, ConnectionId) {
        let connection_id = self.cur_connection_id;
        self.cur_connection_id.0 += 1;

        let (duplex_controller, duplex_mock) = tokio::io::duplex(MAX_DUPLEX_BUFFER_SIZE);
        let (duplex_mock_read, duplex_mock_write) = tokio::io::split(duplex_mock);
        let (duplex_controller_read, duplex_controller_write) = tokio::io::split(duplex_controller);
        self.network_event_tx
            .send(NetworkEvent::NewConnection((
                connection_id,
                duplex_controller_read,
                duplex_controller_write,
            )))
            .await
            .expect("MockNetworkController event channel failed");

        (duplex_mock_read, duplex_mock_write, connection_id)
    }
}
