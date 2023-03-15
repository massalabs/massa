// Copyright (c) 2022 MASSA LABS <info@massa.net>
use async_trait::async_trait;
use massa_time::MassaTime;
use std::{
    io,
    net::{SocketAddr, TcpListener, TcpStream},
};

/// duplex connection
pub type Duplex = tokio::net::TcpStream;

/// Listener used to establish a Duplex
pub type DuplexListener = tokio::net::TcpListener;

/// Specifies a common interface that can be used by standard, or mockers
pub trait BSListener {
    fn accept(&mut self) -> io::Result<(TcpStream, SocketAddr)>;
}

/// Specifies a common interface that can be used by standard, or mockers
pub trait BSConnector {
    fn connect(&mut self, addr: SocketAddr) -> io::Result<TcpStream>;
}

/// Specifies a common interface that can be used by standard, or mockers
pub trait BSEstablisher {
    // TODO: this is needed for thread spawning. Once the listener is on-thread, the static
    // lifetime can be thrown away.
    // TODO: use super-advanced lifetime/GAT/other shenanigans to
    // make the listener compatable with being moved into a thread
    type Listener: BSListener + Send + 'static;
    type Connector: BSConnector;
    fn get_listener(&mut self, addr: SocketAddr) -> io::Result<Self::Listener>;
    fn get_connector(&mut self, timeout_duration: MassaTime) -> io::Result<Self::Connector>;
}

/// The listener we are using
#[derive(Debug)]
pub struct DefaultListener(TcpListener);

impl BSListener for DefaultListener {
    /// Accepts a new incoming connection from this listener.
    fn accept(&mut self) -> io::Result<(TcpStream, SocketAddr)> {
        // accept
        let (sock, mut remote_addr) = self.0.accept()?;
        // normalize address
        remote_addr.set_ip(remote_addr.ip().to_canonical());
        Ok((sock, remote_addr))
    }
}
/// Initiates a connection with given timeout in milliseconds
#[derive(Debug)]
pub struct DefaultConnector(MassaTime);

impl BSConnector for DefaultConnector {
    /// Tries to connect to address
    ///
    /// # Argument
    /// * `addr`: `SocketAddr` we are trying to connect to.
    fn connect(&mut self, addr: SocketAddr) -> io::Result<TcpStream> {
        TcpStream::connect_timeout(&addr, self.0.to_duration())
    }
}

/// Establishes a connection
#[derive(Debug)]
pub struct DefaultEstablisher;

impl DefaultEstablisher {
    /// Creates an Establisher.
    pub fn new() -> Self {
        DefaultEstablisher {}
    }
}

impl BSEstablisher for DefaultEstablisher {
    type Connector = DefaultConnector;
    type Listener = DefaultListener;
    /// Gets the associated listener
    ///
    /// # Argument
    /// * `addr`: `SocketAddr` we want to bind to.
    fn get_listener(&mut self, addr: SocketAddr) -> io::Result<DefaultListener> {
        // Create a socket2 TCP listener to manually set the IPV6_V6ONLY flag
        let socket = socket2::Socket::new(socket2::Domain::IPV6, socket2::Type::STREAM, None)?;

        socket.set_only_v6(false)?;
        socket.set_nonblocking(true)?;
        socket.bind(&addr.into())?;

        // Number of connections to queue, set to the hardcoded value used by tokio
        socket.listen(1024)?;

        Ok(DefaultListener(socket.into()))
    }

    /// Get the connector with associated timeout
    ///
    /// # Argument
    /// * `timeout_duration`: timeout duration in milliseconds
    fn get_connector(&mut self, timeout_duration: MassaTime) -> io::Result<DefaultConnector> {
        Ok(DefaultConnector(timeout_duration))
    }
}

impl Default for DefaultEstablisher {
    fn default() -> Self {
        Self::new()
    }
}
