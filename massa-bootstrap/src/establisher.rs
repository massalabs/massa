// Copyright (c) 2022 MASSA LABS <info@massa.net>
use massa_time::MassaTime;
use std::{
    io,
    net::{SocketAddr, TcpListener, TcpStream},
};

/// duplex connection
pub trait Duplex:
// static because need to send between threads :(
    'static  + Send + io::Read + io::Write
{
}

impl Duplex for std::net::TcpStream {}

/// Specifies a common interface that can be used by standard, or mockers
#[cfg_attr(test, mockall::automock)]
pub trait BSListener {
    fn accept(&mut self) -> io::Result<(TcpStream, SocketAddr)>;
}

/// Specifies a common interface that can be used by standard, or mockers
#[cfg_attr(test, mockall::automock)]
pub trait BSConnector {
    fn connect(&mut self, addr: SocketAddr) -> io::Result<TcpStream>;
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
