use super::establisher::*;
use async_trait::async_trait;
use std::io;
use std::net::SocketAddr;
use time::UTime;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

/// The listener we are using
#[derive(Debug)]
pub struct DefaultListener(TcpListener);

#[async_trait]
impl Listener<OwnedReadHalf, OwnedWriteHalf> for DefaultListener {
    /// Accepts a new incoming connection from this listener.
    async fn accept(&mut self) -> io::Result<(OwnedReadHalf, OwnedWriteHalf, SocketAddr)> {
        let (sock, remote_addr) = self.0.accept().await?;
        let (read_half, write_half) = sock.into_split();
        Ok((read_half, write_half, remote_addr))
    }
}

/// Initiates a connection with given timeout in millis
#[derive(Debug)]
pub struct DefaultConnector(UTime);

#[async_trait]
impl Connector<OwnedReadHalf, OwnedWriteHalf> for DefaultConnector {
    /// Tries to connect to addr
    ///
    /// # Argument
    /// * addr: SocketAddr we are trying to connect to.
    async fn connect(&mut self, addr: SocketAddr) -> io::Result<(OwnedReadHalf, OwnedWriteHalf)> {
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
#[derive(Debug)]
pub struct DefaultEstablisher;

#[async_trait]
impl Establisher for DefaultEstablisher {
    type ReaderT = OwnedReadHalf;
    type WriterT = OwnedWriteHalf;
    type ListenerT = DefaultListener;
    type ConnectorT = DefaultConnector;

    /// Gets the associated listener
    ///
    /// # Argument
    /// * addr: SocketAddr we want to bind to.
    async fn get_listener(&mut self, addr: SocketAddr) -> io::Result<Self::ListenerT> {
        Ok(DefaultListener(TcpListener::bind(addr).await?))
    }

    /// Get the connector with associated timeout
    ///
    /// # Argument
    /// * timeout_duration: timeout duration in millis
    async fn get_connector(&mut self, timeout_duration: UTime) -> io::Result<Self::ConnectorT> {
        Ok(DefaultConnector(timeout_duration))
    }
}

impl DefaultEstablisher {
    /// Creates an Establisher.
    pub fn new() -> Self {
        DefaultEstablisher {}
    }
}
