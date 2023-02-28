// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::config::CHANNEL_SIZE;
use massa_time::MassaTime;
use socket2 as _;
use std::io;
use std::net::SocketAddr;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio::time::timeout;

pub type Duplex = std::net::TcpStream;

pub fn new() -> (MockEstablisher, MockEstablisherInterface) {
    let (connection_listener_tx, connection_listener_rx) =
        crossbeam::channel::bounded::<(SocketAddr, oneshot::Sender<Duplex>)>(CHANNEL_SIZE);

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
    connection_listener_rx:
        crossbeam::channel::Receiver<(SocketAddr, oneshot::Sender<std::net::TcpStream>)>, // (controller, mock)
}

impl MockListener {
    pub fn blocking_accept(&mut self) -> std::io::Result<(Duplex, SocketAddr)> {
        let (_addr, sender) = self.connection_listener_rx.recv().map_err(|_| {
            io::Error::new(
                io::ErrorKind::Other,
                "MockListener accept channel from Establisher closed".to_string(),
            )
        })?;
        let duplex_controller = std::net::TcpListener::bind("localhost:0").unwrap();
        let duplex_mock = Duplex::connect(duplex_controller.local_addr().unwrap()).unwrap();
        let duplex_controller = duplex_controller.accept().unwrap();

        sender.send(duplex_mock).map_err(|_| {
            io::Error::new(
                io::ErrorKind::Other,
                "MockListener accept return oneshot channel to Establisher closed".to_string(),
            )
        })?;

        Ok(duplex_controller)
    }
}

#[derive(Debug)]
pub struct MockConnector {
    connection_connector_tx:
        crossbeam::channel::Sender<(Duplex, SocketAddr, oneshot::Sender<bool>)>,
    timeout_duration: MassaTime,
}

impl MockConnector {
    pub fn connect(&mut self, addr: SocketAddr) -> std::io::Result<Duplex> {
        let duplex_mock = std::net::TcpListener::bind(addr).unwrap();
        let duplex_controller = Duplex::connect(addr).unwrap();
        let duplex_mock = duplex_mock.accept().unwrap();

        // task the controller connection if exist.

        // to see if the connection is accepted
        let (accept_tx, accept_rx) = oneshot::channel::<bool>();

        // manage time-limits
        let start = Instant::now();
        let time_limit = self.timeout_duration.to_duration();

        // send new connection to mock
        self.connection_connector_tx
            .send_timeout((duplex_mock.0, addr, accept_tx), time_limit)
            .map_err(|_err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    "MockConnector connect channel to Establisher closed".to_string(),
                )
            })?;
        let interval = Instant::now().duration_since(start);
        if interval > time_limit {
            return Err(std::io::ErrorKind::TimedOut.into());
        }

        match accept_rx.recv_timeout(time_limit - interval) {
            Ok(_) => Ok(duplex_controller),
            Err(e) => match e {
                oneshot::RecvTimeoutError::Timeout => Err(io::ErrorKind::TimedOut.into()),
                oneshot::RecvTimeoutError::Disconnected => {
                    Err(io::ErrorKind::ConnectionAborted.into())
                }
            },
        }
    }
}

#[derive(Debug)]
pub struct MockEstablisher {
    connection_listener_rx:
        Option<crossbeam::channel::Receiver<(SocketAddr, oneshot::Sender<Duplex>)>>,
    connection_connector_tx:
        crossbeam::channel::Sender<(Duplex, SocketAddr, oneshot::Sender<bool>)>,
}

impl MockEstablisher {
    pub fn get_listener(&mut self, _addr: SocketAddr) -> io::Result<MockListener> {
        Ok(MockListener {
            connection_listener_rx: self
                .connection_listener_rx
                .take()
                .expect("MockEstablisher get_listener called more than once"),
        })
    }

    pub fn get_connector(&mut self, timeout_duration: MassaTime) -> std::io::Result<MockConnector> {
        // create connector stream

        Ok(MockConnector {
            connection_connector_tx: self.connection_connector_tx.clone(),
            timeout_duration,
        })
    }
}

pub struct MockEstablisherInterface {
    connection_listener_tx:
        Option<crossbeam::channel::Sender<(SocketAddr, oneshot::Sender<Duplex>)>>,
    connection_connector_rx:
        crossbeam::channel::Receiver<(Duplex, SocketAddr, oneshot::Sender<bool>)>,
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
        sender.send((*addr, response_tx)).map_err(|_err| {
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
