// Copyright (c) 2022 MASSA LABS <info@massa.net>

#[cfg(not(test))]
use massa_time::MassaTime;
#[cfg(not(test))]
use std::{io, net::SocketAddr};
#[cfg(not(test))]
use tokio::{
    net::{TcpListener, TcpStream},
    time::timeout,
};

#[cfg(test)]
pub type Duplex = super::tests::mock_establisher::Duplex;
#[cfg(test)]
pub type Listener = super::tests::mock_establisher::MockListener;
#[cfg(test)]
pub type Connector = super::tests::mock_establisher::MockConnector;
#[cfg(test)]
pub type Establisher = super::tests::mock_establisher::MockEstablisher;

#[cfg(not(test))]
pub type Duplex = TcpStream;
#[cfg(not(test))]
pub type Listener = DefaultListener;
#[cfg(not(test))]
pub type Connector = DefaultConnector;
#[cfg(not(test))]
pub type Establisher = DefaultEstablisher;

/// The listener we are using
#[cfg(not(test))]
#[derive(Debug)]
pub struct DefaultListener(TcpListener);

#[cfg(not(test))]
impl DefaultListener {
    /// Accepts a new incoming connection from this listener.
    pub async fn accept(&mut self) -> io::Result<(Duplex, SocketAddr)> {
        let (sock, remote_addr) = self.0.accept().await?;
        Ok((sock, remote_addr))
    }
}

/// Initiates a connection with given timeout in millis
#[cfg(not(test))]
#[derive(Debug)]
pub struct DefaultConnector(MassaTime);

#[cfg(not(test))]
impl DefaultConnector {
    /// Tries to connect to address
    ///
    /// # Argument
    /// * addr: SocketAddr we are trying to connect to.
    pub async fn connect(&mut self, addr: SocketAddr) -> io::Result<Duplex> {
        match timeout(self.0.to_duration(), TcpStream::connect(addr)).await {
            Ok(Ok(sock)) => Ok(sock),
            Ok(Err(e)) => Err(e),
            Err(e) => Err(io::Error::new(io::ErrorKind::TimedOut, e)),
        }
    }
}

/// Establishes a connection
#[cfg(not(test))]
#[derive(Debug)]
pub struct DefaultEstablisher;

#[cfg(not(test))]
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

#[cfg(not(test))]
impl Default for DefaultEstablisher {
    fn default() -> Self {
        Self::new()
    }
}
