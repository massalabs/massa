// Copyright (c) 2022 MASSA LABS <info@massa.net>
use std::{io, net::SocketAddr};

#[cfg(test)]
pub mod types {
    pub type Listener = crate::tests::mock_establisher::MockListener;

    pub type Connector = crate::tests::mock_establisher::MockConnector;

    pub type Establisher = crate::tests::mock_establisher::MockEstablisher;
/// duplex connection
pub type Duplex = tokio::net::TcpStream;
/// duplex connection
pub type DuplexListener = tokio::net::TcpListener;

}

#[cfg(not(test))]
/// Connection types
pub mod types {
    use massa_time::MassaTime;

    use super::{io, Duplex, DuplexListener, SocketAddr};

    /// connector, used by client
    pub type Connector = DefaultConnector;
    /// connection establisher
    pub type Establisher = DefaultEstablisher;

    /// The listener we are using
    #[derive(Debug)]
    pub struct DefaultListener(DuplexListener);

    impl DefaultListener {
        /// Accepts a new incoming connection from this listener.
        pub async fn accept(&mut self) -> io::Result<(Duplex, SocketAddr)> {
            // accept
            let (sock, mut remote_addr) = self.0.accept().await?;
            // normalize address
            remote_addr.set_ip(remote_addr.ip().to_canonical());
            Ok((sock, remote_addr))
        }
    }

    /// Initiates a connection with given timeout in milliseconds
    #[derive(Debug)]
    pub struct DefaultConnector(MassaTime);

    impl DefaultConnector {
        /// Tries to connect to address
        ///
        /// # Argument
        /// * `addr`: `SocketAddr` we are trying to connect to.
        pub async fn connect(&mut self, addr: SocketAddr) -> io::Result<Duplex> {
            match tokio::time::timeout(self.0.to_duration(), Duplex::connect(addr)).await {
                Ok(Ok(sock)) => Ok(sock),
                Ok(Err(e)) => Err(e),
                Err(e) => Err(io::Error::new(io::ErrorKind::TimedOut, e)),
            }
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

        /// Gets the associated listener
        ///
        /// # Argument
        /// * `addr`: `SocketAddr` we want to bind to.
        pub fn get_listener(&mut self, addr: SocketAddr) -> io::Result<DefaultListener> {
            // Create a socket2 TCP listener to manually set the IPV6_V6ONLY flag
            let socket = socket2::Socket::new(socket2::Domain::IPV6, socket2::Type::STREAM, None)?;

            socket.set_only_v6(false)?;
            socket.set_nonblocking(true)?;
            socket.bind(&addr.into())?;

            // Number of connections to queue, set to the hardcoded value used by tokio
            socket.listen(1024)?;

            Ok(DefaultListener(DuplexListener::from_std(socket.into())?))
        }

        /// Get the connector with associated timeout
        ///
        /// # Argument
        /// * `timeout_duration`: timeout duration in milliseconds
        pub fn get_connector(
            &mut self,
            timeout_duration: MassaTime,
        ) -> io::Result<DefaultConnector> {
            Ok(DefaultConnector(timeout_duration))
        }
    }

    impl Default for DefaultEstablisher {
        fn default() -> Self {
            Self::new()
        }
    }
}
