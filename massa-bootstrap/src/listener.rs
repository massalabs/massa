use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Token, Waker};
use std::io::ErrorKind;
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;
use tracing::{info, warn};

use crate::error::BootstrapError;
use crate::tools::mio_stream_to_std;

const NEW_CONNECTION: Token = Token(0);
const STOP_LISTENER: Token = Token(10);

/// Maximum number of connections accepted in a single `poll` cycle.
///
/// Draining the whole pending backlog at once would let a connection flood move
/// the entire listen queue (1024 entries) into process-owned file descriptors
/// before the server has had any chance to apply admission control, and would
/// hand the server a batch of that size to refuse in one go. Accepting in
/// bounded batches keeps the burst small; the remainder stays in the kernel
/// backlog and is picked up by the following cycles.
const MAX_ACCEPTS_PER_POLL: usize = 32;

/// TODO: this should be crate-private. currently needed for models testing
pub struct BootstrapTcpListener {
    poll: Poll,
    events: Events,
    server: TcpListener,
    /// Set when the last drain stopped at [`MAX_ACCEPTS_PER_POLL`]. The mio
    /// registration is edge-triggered, so a backlog left behind does not
    /// re-arm readiness on its own: the next `poll` has to look for it instead
    /// of blocking.
    backlog_pending: bool,
}

pub struct BootstrapListenerStopHandle(pub(crate) Waker);

/// Drain at most `max` currently-ready connections from `accept` into `out`.
///
/// The drain ends on the first error, whether it is `WouldBlock` (no more
/// pending connections) or any other error such as a persistent `EMFILE` /
/// `ENFILE`. Ending the drain on a non-`WouldBlock` error (instead of
/// `continue`-ing) is what prevents a sticky listener-level failure from
/// spinning `poll()` forever and starving the `STOP_LISTENER` token.
///
/// Returns `true` if the drain stopped because `max` was reached, meaning the
/// backlog may still hold connections that the caller has to come back for.
fn drain_accept<S>(
    mut accept: impl FnMut() -> std::io::Result<S>,
    out: &mut Vec<S>,
    max: usize,
) -> bool {
    for _ in 0..max {
        match accept() {
            Ok(item) => out.push(item),
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => return false,
            Err(e) => {
                warn!("Error accepting connection in bootstrap: {:?}", e);
                return false;
            }
        }
    }
    true
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
                backlog_pending: false,
            },
        ))
    }

    /// Poll the listener for new connections
    ///
    /// At most [`MAX_ACCEPTS_PER_POLL`] connections are returned per call, so
    /// that the caller gets to apply admission control before the rest of the
    /// backlog is pulled into the process.
    pub fn poll(&mut self) -> Result<PollEvent, BootstrapError> {
        // Only block when the backlog is known to be empty: a backlog left over
        // from the previous cycle would otherwise wait for an unrelated
        // readiness event before being served.
        let timeout = if self.backlog_pending {
            Some(Duration::ZERO)
        } else {
            None
        };
        self.poll.poll(&mut self.events, timeout).unwrap();

        let mut accept_ready = self.backlog_pending;
        for event in self.events.iter() {
            match event.token() {
                NEW_CONNECTION => accept_ready = true,
                STOP_LISTENER => {
                    return Ok(PollEvent::Stop);
                }
                _ => unreachable!(),
            }
        }

        if !accept_ready {
            return Ok(PollEvent::NewConnections(Vec::new()));
        }

        // Drain the ready connections, borrowing only `self.server`, then
        // post-process them (which borrows `self.poll`). The drain never spins
        // on a persistent accept error, so control always returns to the poller
        // and the STOP token can be serviced.
        let mut accepted = Vec::with_capacity(MAX_ACCEPTS_PER_POLL);
        self.backlog_pending =
            drain_accept(|| self.server.accept(), &mut accepted, MAX_ACCEPTS_PER_POLL);

        let mut results = Vec::with_capacity(accepted.len());
        for (mut stream, remote_addr) in accepted {
            let _ = self.poll.registry().deregister(&mut stream);
            let stream: std::net::TcpStream = mio_stream_to_std(stream);
            stream.set_nonblocking(false)?;
            results.push((stream, remote_addr));
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
    use super::{drain_accept, MAX_ACCEPTS_PER_POLL};
    use std::io::{Error, ErrorKind};

    #[test]
    fn drain_stops_on_persistent_error_instead_of_spinning() {
        let mut calls = 0u32;
        let mut out: Vec<u8> = Vec::new();
        let capped = drain_accept(
            || {
                calls += 1;
                // A persistent non-WouldBlock error (e.g. EMFILE/ENFILE).
                Err::<u8, _>(Error::from(ErrorKind::Other))
            },
            &mut out,
            MAX_ACCEPTS_PER_POLL,
        );
        assert_eq!(
            calls, 1,
            "a persistent accept error must break the drain, not loop forever"
        );
        assert!(out.is_empty());
        assert!(!capped, "an errored drain leaves no known backlog");
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
        let capped = drain_accept(|| seq.next().unwrap(), &mut out, MAX_ACCEPTS_PER_POLL);
        // Stops at the WouldBlock, leaving the trailing item untouched.
        assert_eq!(out, vec![1, 2]);
        assert!(!capped, "a drained backlog must not be reported as pending");
    }

    #[test]
    fn drain_stops_at_the_batch_limit() {
        // An endless supply of pending connections, as a backlog flood would.
        let mut accepted = 0usize;
        let mut out = Vec::new();
        let capped = drain_accept(
            || {
                accepted += 1;
                Ok::<u8, Error>(0)
            },
            &mut out,
            MAX_ACCEPTS_PER_POLL,
        );
        assert_eq!(out.len(), MAX_ACCEPTS_PER_POLL);
        assert_eq!(
            accepted, MAX_ACCEPTS_PER_POLL,
            "the drain must not accept beyond the batch limit"
        );
        assert!(capped, "a capped drain must report the remaining backlog");
    }
}
