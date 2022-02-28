// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::constants::{CHANNEL_SIZE, MAX_DUPLEX_BUFFER_SIZE};
use massa_time::MassaTime;
use std::io;
use std::net::SocketAddr;
use tokio::io::DuplexStream;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

pub type Duplex = DuplexStream;

pub fn new() -> (MockEstablisher, MockEstablisherInterface) {
    let (connection_listener_tx, connection_listener_rx) =
        mpsc::channel::<(SocketAddr, oneshot::Sender<Duplex>)>(CHANNEL_SIZE);

    let (connection_connector_tx, connection_connector_rx) =
        mpsc::channel::<(Duplex, SocketAddr, oneshot::Sender<bool>)>(CHANNEL_SIZE);

    (
        MockEstablisher {
            connection_listener_rx: Some(connection_listener_rx),
            connection_connector_tx,
        },
        MockEstablisherInterface {
            connection_listener_tx: Some(connection_listener_tx),
            connection_connector_rx,
        },
    )
}

#[derive(Debug)]
pub struct MockListener {
    connection_listener_rx: mpsc::Receiver<(SocketAddr, oneshot::Sender<Duplex>)>, // (controller, mock)
}

impl MockListener {
    pub async fn accept(&mut self) -> std::io::Result<(Duplex, SocketAddr)> {
        let (addr, sender) = self.connection_listener_rx.recv().await.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Other,
                "MockListener accept channel from Establisher closed".to_string(),
            )
        })?;
        let (duplex_controller, duplex_mock) = tokio::io::duplex(MAX_DUPLEX_BUFFER_SIZE);
        sender.send(duplex_mock).map_err(|_| {
            io::Error::new(
                io::ErrorKind::Other,
                "MockListener accept return oneshot channel to Establisher closed".to_string(),
            )
        })?;

        Ok((duplex_controller, addr))
    }
}

#[derive(Debug)]
pub struct MockConnector {
    connection_connector_tx: mpsc::Sender<(Duplex, SocketAddr, oneshot::Sender<bool>)>,
    timeout_duration: MassaTime,
}

impl MockConnector {
    pub async fn connect(&mut self, addr: SocketAddr) -> std::io::Result<Duplex> {
        // task the controller connection if exist.
        let (duplex_controller, duplex_mock) = tokio::io::duplex(MAX_DUPLEX_BUFFER_SIZE);
        // to see if the connection is accepted
        let (accept_tx, accept_rx) = oneshot::channel::<bool>();

        // send new connection to mock
        timeout(self.timeout_duration.to_duration(), async move {
            self.connection_connector_tx
                .send((duplex_mock, addr, accept_tx))
                .await
                .map_err(|_err| {
                    io::Error::new(
                        io::ErrorKind::Other,
                        "MockConnector connect channel to Establisher closed".to_string(),
                    )
                })?;
            if accept_rx.await.expect("mock accept_tx disappeared") {
                Ok(duplex_controller)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::ConnectionRefused,
                    "mock refused the connection".to_string(),
                ))
            }
        })
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "MockConnector connection attempt timed out".to_string(),
            )
        })?
    }
}

#[derive(Debug)]
pub struct MockEstablisher {
    connection_listener_rx: Option<mpsc::Receiver<(SocketAddr, oneshot::Sender<Duplex>)>>,
    connection_connector_tx: mpsc::Sender<(Duplex, SocketAddr, oneshot::Sender<bool>)>,
}

impl MockEstablisher {
    pub async fn get_listener(&mut self, _addr: SocketAddr) -> io::Result<MockListener> {
        Ok(MockListener {
            connection_listener_rx: self
                .connection_listener_rx
                .take()
                .expect("MockEstablisher get_listener called more than once"),
        })
    }

    pub async fn get_connector(
        &mut self,
        timeout_duration: MassaTime,
    ) -> std::io::Result<MockConnector> {
        // create connector stream

        Ok(MockConnector {
            connection_connector_tx: self.connection_connector_tx.clone(),
            timeout_duration,
        })
    }
}

pub struct MockEstablisherInterface {
    connection_listener_tx: Option<mpsc::Sender<(SocketAddr, oneshot::Sender<Duplex>)>>,
    connection_connector_rx: mpsc::Receiver<(Duplex, SocketAddr, oneshot::Sender<bool>)>,
}

impl MockEstablisherInterface {
    pub async fn connect_to_controller(&self, addr: &SocketAddr) -> io::Result<Duplex> {
        let sender = self.connection_listener_tx.as_ref().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Other,
                "mock connect_to_controller_listener channel not initialized".to_string(),
            )
        })?;
        let (response_tx, response_rx) = oneshot::channel::<Duplex>();
        sender.send((*addr, response_tx)).await.map_err(|_err| {
            io::Error::new(
                io::ErrorKind::Other,
                "mock connect_to_controller_listener channel to listener closed".to_string(),
            )
        })?;
        let duplex_mock = response_rx.await.map_err(|_| {
            io::Error::new(
                io::ErrorKind::Other,
                "MockListener connect_to_controller_listener channel from listener closed"
                    .to_string(),
            )
        })?;
        Ok(duplex_mock)
    }

    pub async fn wait_connection_attempt_from_controller(
        &mut self,
    ) -> io::Result<(Duplex, SocketAddr, oneshot::Sender<bool>)> {
        self.connection_connector_rx.recv().await.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Other,
                "MockListener get_connect_stream channel from connector closed".to_string(),
            )
        })
    }
}
