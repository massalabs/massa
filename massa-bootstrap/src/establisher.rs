// Copyright (c) 2022 MASSA LABS <info@massa.net>
use massa_time::MassaTime;
use mio::{
    net::{TcpListener, TcpStream as MioTcpStream},
    Events, Poll, Token,
};
use std::{
    io,
    net::{SocketAddr, TcpStream},
};

/// Specifies a common interface that can be used by standard, or mockers
#[cfg_attr(test, mockall::automock)]
pub trait BSListener {
    fn accept(&mut self) -> io::Result<(MioTcpStream, SocketAddr)>;
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
pub struct DefaultListener {
    poll: Poll,
    events: Events,
    listener: TcpListener,
}

const ACCEPT_EV: Token = Token(0);
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
        socket.set_nonblocking(true)?;
        let std_sock = socket.into();
        let mut mio_listener = mio::net::TcpListener::from_std(std_sock);

        let poll = Poll::new()?;
        let events = Events::with_capacity(1024);
        poll.registry()
            .register(&mut mio_listener, ACCEPT_EV, mio::Interest::READABLE)?;

        Ok(DefaultListener {
            listener: mio_listener,
            poll,
            events,
        })
    }
}

impl BSListener for DefaultListener {
    /// Accepts a new incoming connection from this listener.
    fn accept(&mut self) -> io::Result<(MioTcpStream, SocketAddr)> {
        loop {
            self.poll.poll(&mut self.events, None)?;
            for event in self.events.iter() {
                if event.token() == ACCEPT_EV {
                    let (sock, mut remote_addr) = self.listener.accept()?;

                    // normalize address
                    remote_addr.set_ip(remote_addr.ip().to_canonical());
                    return Ok((sock, remote_addr));
                }
            }
        }
        // accept
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
