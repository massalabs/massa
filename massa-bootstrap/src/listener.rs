use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Token, Waker};
use std::io::ErrorKind;
use std::net::{SocketAddr, TcpStream};
use tracing::{info, warn};

use crate::error::BootstrapError;
use crate::tools::mio_stream_to_std;

const NEW_CONNECTION: Token = Token(0);
const STOP_LISTENER: Token = Token(10);

/// TODO: this should be crate-private. currently needed for models testing
pub struct BootstrapTcpListener {
    poll: Poll,
    events: Events,
    server: TcpListener,
}

pub struct BootstrapListenerStopHandle(pub(crate) Waker);

/// Drain all currently-ready connections from `accept` into `out`.
///
/// The drain ends on the first error, whether it is `WouldBlock` (no more
/// pending connections) or any other error such as a persistent `EMFILE` /
/// `ENFILE`. Ending the drain on a non-`WouldBlock` error (instead of
/// `continue`-ing) is what prevents a sticky listener-level failure from
/// spinning `poll()` forever and starving the `STOP_LISTENER` token.
fn drain_accept<S>(mut accept: impl FnMut() -> std::io::Result<S>, out: &mut Vec<S>) {
    loop {
        match accept() {
            Ok(item) => out.push(item),
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) => {
                warn!("Error accepting connection in bootstrap: {:?}", e);
                break;
            }
        }
    }
}

pub enum PollEvent {
    NewConnections(Vec<(TcpStream, SocketAddr)>),
    Stop,
}

#[cfg_attr(test, mockall::automock)]
impl BootstrapTcpListener {
    /// Setup a mio-listener that functions as a `select!` on a connection, or a waker
    ///
    /// * `addr` - the address to listen on
    pub fn create(
        addr: &SocketAddr,
    ) -> Result<(BootstrapListenerStopHandle, Self), BootstrapError> {
        let domain = if addr.is_ipv4() {
            socket2::Domain::IPV4
        } else {
            socket2::Domain::IPV6
        };

        let socket = socket2::Socket::new(domain, socket2::Type::STREAM, None)?;

        if addr.is_ipv6() {
            socket.set_only_v6(false)?;
        }
        // This is needed for the mio-polling system, which depends on the socket being non-blocking.
        // If we don't set non-blocking, then we can .accept() on the server below, which is needed to ensure the polling triggers every time.
        socket.set_nonblocking(true)?;
        socket.bind(&(*addr).into())?;

        // Number of connections to queue, set to the hardcoded value used by tokio
        socket.listen(1024)?;

        info!("Starting bootstrap listener on {}", &addr);
        let mut server = TcpListener::from_std(socket.into());

        let poll = Poll::new()?;

        // wake up the poll when we want to stop the listener
        let waker = BootstrapListenerStopHandle(Waker::new(poll.registry(), STOP_LISTENER)?);

        poll.registry()
            .register(&mut server, NEW_CONNECTION, Interest::READABLE)?;

        // TODO use config for capacity ?
        let events = Events::with_capacity(128);
        Ok((
            waker,
            BootstrapTcpListener {
                poll,
                server,
                events,
            },
        ))
    }

    /// Poll the listener for new connections
    pub fn poll(&mut self) -> Result<PollEvent, BootstrapError> {
        self.poll.poll(&mut self.events, None).unwrap();

        let mut results = Vec::with_capacity(self.events.iter().count());

        // Process each event.
        for event in self.events.iter() {
            match event.token() {
                NEW_CONNECTION => {
                    // Drain the currently-ready connections, borrowing only
                    // `self.server`, then post-process them (which borrows
                    // `self.poll`). The drain never spins on a persistent
                    // accept error, so control always returns to the poller and
                    // the STOP token can be serviced.
                    let mut accepted = Vec::new();
                    drain_accept(|| self.server.accept(), &mut accepted);
                    for (mut stream, remote_addr) in accepted {
                        let _ = self.poll.registry().deregister(&mut stream);
                        let stream: std::net::TcpStream = mio_stream_to_std(stream);
                        stream.set_nonblocking(false)?;
                        results.push((stream, remote_addr));
                    }
                }
                STOP_LISTENER => {
                    return Ok(PollEvent::Stop);
                }
                _ => unreachable!(),
            }
        }

        Ok(PollEvent::NewConnections(results))
    }
}

impl BootstrapListenerStopHandle {
    /// Stop the bootstrap listener.
    pub fn stop(&self) -> Result<(), BootstrapError> {
        self.0.wake().map_err(BootstrapError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::drain_accept;
    use std::io::{Error, ErrorKind};

    #[test]
    fn drain_stops_on_persistent_error_instead_of_spinning() {
        let mut calls = 0u32;
        let mut out: Vec<u8> = Vec::new();
        drain_accept(
            || {
                calls += 1;
                // A persistent non-WouldBlock error (e.g. EMFILE/ENFILE).
                Err::<u8, _>(Error::from(ErrorKind::Other))
            },
            &mut out,
        );
        assert_eq!(
            calls, 1,
            "a persistent accept error must break the drain, not loop forever"
        );
        assert!(out.is_empty());
    }

    #[test]
    fn drain_collects_ready_connections_until_would_block() {
        let mut seq = vec![
            Ok(1u8),
            Ok(2u8),
            Err(Error::from(ErrorKind::WouldBlock)),
            Ok(3u8),
        ]
        .into_iter();
        let mut out = Vec::new();
        drain_accept(|| seq.next().unwrap(), &mut out);
        // Stops at the WouldBlock, leaving the trailing item untouched.
        assert_eq!(out, vec![1, 2]);
    }
}
