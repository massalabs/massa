// Copyright (c) 2022 MASSA LABS <info@massa.net>
use massa_time::MassaTime;
use std::{
    io,
    net::{SocketAddr, TcpListener, TcpStream},
    time::Duration,
};

/// duplex connection
pub trait Duplex:
// static because need to send between threads :(
    'static  + Send + io::Read + io::Write
{
    fn set_read_timeout(&mut self, duration: Option<Duration>) -> io::Result<()>;
    fn set_write_timeout(&mut self, duration: Option<Duration>) -> io::Result<()>;
    fn set_nonblocking(&mut self, setting: bool) -> io::Result<()>;
}

impl Duplex for std::net::TcpStream {
    fn set_read_timeout(&mut self, duration: Option<Duration>) -> io::Result<()> {
        Self::set_read_timeout(&self, duration)
    }

    fn set_write_timeout(&mut self, duration: Option<Duration>) -> io::Result<()> {
        Self::set_write_timeout(&self, duration)
    }

    fn set_nonblocking(&mut self, setting: bool) -> io::Result<()> {
        Self::set_nonblocking(&self, setting)
    }
}

/// Specifies a common interface that can be used by standard, or mockers
#[cfg_attr(test, mockall::automock)]
pub trait BSListener {
    fn accept(&mut self) -> io::Result<(TcpStream, SocketAddr)>;
}

/// Specifies a common interface that can be used by standard, or mockers
#[cfg_attr(test, mockall::automock)]
pub trait BSConnector {
    fn connect_timeout(
        &self,
        addr: SocketAddr,
        duration: Option<MassaTime>,
    ) -> io::Result<TcpStream>;
}

/// The listener we are using
#[derive(Debug)]
pub struct DefaultListener(TcpListener);
impl DefaultListener {
    /// Provides a standard TcpListener
    pub fn new(addr: &SocketAddr) -> io::Result<DefaultListener> {
        // Create a socket2 TCP listener to manually set the IPV6_V6ONLY flag
        // This is needed to get the same behavior on all OS
        // However, if IPv6 is disabled system-wide, you may need to bind to an IPv4 address instead.
        let domain = match addr.is_ipv4() {
            true => socket2::Domain::IPV4,
            _ => socket2::Domain::IPV6,
        };

        let socket = socket2::Socket::new(domain, socket2::Type::STREAM, None)?;

        if addr.is_ipv6() {
            socket.set_only_v6(false)?;
        }
        socket.bind(&(*addr).into())?;

        // Number of connections to queue, set to the hardcoded value used by tokio
        socket.listen(1024)?;
        socket.set_nonblocking(false);
        Ok(DefaultListener(socket.into()))
    }
}

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
pub struct DefaultConnector;

impl BSConnector for DefaultConnector {
    /// Tries to connect to address
    ///
    /// # Argument
    /// * `addr`: `SocketAddr` we are trying to connect to.
    fn connect_timeout(
        &self,
        addr: SocketAddr,
        duration: Option<MassaTime>,
    ) -> io::Result<TcpStream> {
        let Some(duration) = duration else {
            return TcpStream::connect(&addr);
        };
        TcpStream::connect_timeout(&addr, duration.to_duration())
    }
}
