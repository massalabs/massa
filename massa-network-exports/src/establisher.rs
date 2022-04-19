// Copyright (c) 2022 MASSA LABS <info@massa.net>
#[cfg(feature = "testing")]
mod types {
    use crate::test_exports::mock_establisher;

    /// read half of the duplex stream
    pub type ReadHalf = mock_establisher::ReadHalf;
    /// write half of the duplex stream
    pub type WriteHalf = mock_establisher::WriteHalf;
    /// connection listener
    pub type Listener = mock_establisher::MockListener;
    /// connection establisher
    pub type Establisher = mock_establisher::MockEstablisher;
}
#[cfg(not(feature = "testing"))]
mod types {
    use massa_time::MassaTime;
    use std::{io, net::SocketAddr};
    use tokio::{
        net::{TcpListener, TcpStream},
        time::timeout,
    };

    /// read half of the TCP stream
    pub type ReadHalf = tokio::net::tcp::OwnedReadHalf;
    /// Write half of the TCP stream
    pub type WriteHalf = tokio::net::tcp::OwnedWriteHalf;
    /// Connection listener
    pub type Listener = DefaultListener;
    /// Connection establisher
    pub type Establisher = DefaultEstablisher;

    /// The listener we are using
    #[derive(Debug)]
    pub struct DefaultListener(TcpListener);

    impl DefaultListener {
        /// Accepts a new incoming connection from this listener.
        pub async fn accept(&mut self) -> io::Result<(ReadHalf, WriteHalf, SocketAddr)> {
            // accept
            let (sock, mut remote_addr) = self.0.accept().await?;
            // normalize IP
            remote_addr.set_ip(remote_addr.ip().to_canonical());
            // split into read half and write half
            let (read_half, write_half) = sock.into_split();
            Ok((read_half, write_half, remote_addr))
        }
    }

    /// Initiates a connection with given timeout in milliseconds
    #[derive(Debug)]
    pub struct DefaultConnector(MassaTime);

    impl DefaultConnector {
        /// Tries to connect to a address
        ///
        /// # Argument
        /// * `addr`: `SocketAddr` we are trying to connect to.
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
        pub async fn get_listener(&mut self, addr: SocketAddr) -> io::Result<DefaultListener> {
            Ok(DefaultListener(TcpListener::bind(addr).await?))
        }

        /// Get the connector with associated timeout
        ///
        /// # Argument
        /// *` timeout_duration`: timeout duration in milliseconds
        pub async fn get_connector(
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

pub use types::*;
