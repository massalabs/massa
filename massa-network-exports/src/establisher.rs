// Copyright (c) 2022 MASSA LABS <info@massa.net>

#[cfg(not(feature = "testing"))]
use massa_time::MassaTime;
#[cfg(not(feature = "testing"))]
use std::{io, net::SocketAddr};
#[cfg(not(feature = "testing"))]
use tokio::{
    net::{TcpListener, TcpStream},
    time::timeout,
};

#[cfg(feature = "testing")]
pub type ReadHalf = super::test_exports::mock_establisher::ReadHalf;
#[cfg(feature = "testing")]
pub type WriteHalf = super::test_exports::mock_establisher::WriteHalf;
#[cfg(feature = "testing")]
pub type Listener = super::test_exports::mock_establisher::MockListener;
#[cfg(feature = "testing")]
pub type Connector = super::test_exports::mock_establisher::MockConnector;
#[cfg(feature = "testing")]
pub type Establisher = super::test_exports::mock_establisher::MockEstablisher;

#[cfg(not(feature = "testing"))]
pub type ReadHalf = tokio::net::tcp::OwnedReadHalf;
#[cfg(not(feature = "testing"))]
pub type WriteHalf = tokio::net::tcp::OwnedWriteHalf;
#[cfg(not(feature = "testing"))]
pub type Listener = DefaultListener;
#[cfg(not(feature = "testing"))]
pub type Connector = DefaultConnector;
#[cfg(not(feature = "testing"))]
pub type Establisher = DefaultEstablisher;

/// The listener we are using
#[cfg(not(feature = "testing"))]
#[derive(Debug)]
pub struct DefaultListener(TcpListener);

#[cfg(not(feature = "testing"))]
impl DefaultListener {
    /// Accepts a new incoming connection from this listener.
    pub async fn accept(&mut self) -> io::Result<(ReadHalf, WriteHalf, SocketAddr)> {
        let (sock, remote_addr) = self.0.accept().await?;
        let (read_half, write_half) = sock.into_split();
        Ok((read_half, write_half, remote_addr))
    }
}

/// Initiates a connection with given timeout in millis
#[cfg(not(feature = "testing"))]
#[derive(Debug)]
pub struct DefaultConnector(MassaTime);

#[cfg(not(feature = "testing"))]
impl DefaultConnector {
    /// Tries to connect to addr
    ///
    /// # Argument
    /// * addr: SocketAddr we are trying to connect to.
    pub async fn connect(&mut self, addr: SocketAddr) -> io::Result<(ReadHalf, WriteHalf)> {
        match timeout(self.0.to_duration(), TcpStream::connect(addr)).await {
            Ok(Ok(sock)) => {
                let (reader, writer) = sock.into_split();
                Ok((reader, writer))
            }
            Ok(Err(e)) => Err(e),
            Err(e) => Err(io::Error::new(io::ErrorKind::TimedOut, e)),
        }
    }
}

/// Establishes a connection
#[cfg(not(feature = "testing"))]
#[derive(Debug)]
pub struct DefaultEstablisher;

#[cfg(not(feature = "testing"))]
impl DefaultEstablisher {
    /// Creates an Establisher.
    pub fn new() -> Self {
        DefaultEstablisher {}
    }

    /// Gets the associated listener
    ///
    /// # Argument
    /// * addr: SocketAddr we want to bind to.
    pub async fn get_listener(&mut self, addr: SocketAddr) -> io::Result<DefaultListener> {
        Ok(DefaultListener(TcpListener::bind(addr).await?))
    }

    /// Get the connector with associated timeout
    ///
    /// # Argument
    /// * timeout_duration: timeout duration in millis
    pub async fn get_connector(
        &mut self,
        timeout_duration: MassaTime,
    ) -> io::Result<DefaultConnector> {
        Ok(DefaultConnector(timeout_duration))
    }
}

#[cfg(not(feature = "testing"))]
impl Default for DefaultEstablisher {
    fn default() -> Self {
        Self::new()
    }
}
