// Copyright (c) 2022 MASSA LABS <info@massa.net>
use massa_time::MassaTime;
use std::{
    io,
    net::{SocketAddr, TcpListener, TcpStream},
};

use crate::{error::BootstrapError, listener::PollEvent};

/// Specifies a common interface that can be used by standard, or mockers
#[cfg_attr(test, mockall::automock)]
pub trait BSEventPoller {
    fn poll(&mut self) -> Result<PollEvent, BootstrapError>;
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
        socket.set_nonblocking(false)?;
        Ok(DefaultListener(socket.into()))
    }
}

impl BSEventPoller for DefaultListener {
    /// Accepts a new incoming connection from this listener.
    fn poll(&mut self) -> Result<PollEvent, BootstrapError> {
        // accept
        let (sock, mut remote_addr) = self.0.accept()?;
        // normalize address
        remote_addr.set_ip(remote_addr.ip().to_canonical());
        Ok(PollEvent::NewConnection((sock, remote_addr)))
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
            return TcpStream::connect(addr);
        };
        TcpStream::connect_timeout(&addr, duration.to_duration())
    }
}
