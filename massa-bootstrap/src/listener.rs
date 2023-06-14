use std::net::{SocketAddr, TcpListener, TcpStream};

use mio::net::TcpListener as MioTcpListener;

use mio::{Events, Interest, Poll, Token, Waker};
use tracing::{debug, info};

use crate::error::BootstrapError;
use crate::server::BSEventPoller;

const NEW_CONNECTION: Token = Token(0);
const STOP_LISTENER: Token = Token(10);

/// TODO: this should be crate-private. currently needed for models testing
pub struct BootstrapTcpListener {
    poll: Poll,
    events: Events,
    server: TcpListener,
    // HACK : create variable to move ownership of mio_server to the thread
    // if mio_server is not moved, poll does not receive any event from listener
    _mio_server: MioTcpListener,
}

pub struct BootstrapListenerStopHandle(Waker);

pub enum PollEvent {
    NewConnection((TcpStream, SocketAddr)),
    Stop,
}
impl BootstrapTcpListener {
    /// Setup a mio-listener that functions as a `select!` on a connection, or a waker
    ///
    /// * `addr` - the address to listen on
    pub fn new(addr: &SocketAddr) -> Result<(BootstrapListenerStopHandle, Self), BootstrapError> {
        let domain = if addr.is_ipv4() {
            socket2::Domain::IPV4
        } else {
            socket2::Domain::IPV6
        };

        let socket = socket2::Socket::new(domain, socket2::Type::STREAM, None)?;

        if addr.is_ipv6() {
            socket.set_only_v6(false)?;
        }
        socket.set_nonblocking(true)?;
        socket.bind(&(*addr).into())?;

        // Number of connections to queue, set to the hardcoded value used by tokio
        socket.listen(1024)?;

        info!("Starting bootstrap listener on {}", &addr);
        let server: TcpListener = socket.into();

        let mut mio_server =
            MioTcpListener::from_std(server.try_clone().expect("Unable to clone server socket"));

        let poll = Poll::new()?;

        // wake up the poll when we want to stop the listener
        let waker = BootstrapListenerStopHandle(Waker::new(poll.registry(), STOP_LISTENER)?);

        poll.registry()
            .register(&mut mio_server, NEW_CONNECTION, Interest::READABLE)?;

        // TODO use config for capacity ?
        let events = Events::with_capacity(32);
        Ok((
            waker,
            BootstrapTcpListener {
                poll,
                server,
                events,
                _mio_server: mio_server,
            },
        ))
    }
}

impl BSEventPoller for BootstrapTcpListener {
    fn poll(&mut self) -> Result<Vec<PollEvent>, BootstrapError> {
        self.poll.poll(&mut self.events, None).unwrap();

        // Confirm that we are not being signalled to shut down
        if self.events.iter().any(|ev| ev.token() == STOP_LISTENER) {
            return Ok(vec![PollEvent::Stop]);
        }

        let mut results = Vec::new();

        // Process each event.
        for event in self.events.iter() {
            match event.token() {
                NEW_CONNECTION => {
                    results.push(PollEvent::NewConnection(self.server.accept()?));
                }
                _ => unreachable!(),
            }
        }

        // We need to have an accept() error with WouldBlock, otherwise polling may not raise any new events.
        // See https://users.rust-lang.org/t/why-mio-poll-only-receives-the-very-first-event/87501
        if self._mio_server.accept().is_ok() {
            debug!("Mio server still had bootstrap connection data to read");
        }

        Ok(results)
    }
}

impl BootstrapListenerStopHandle {
    /// Stop the bootstrap listener.
    pub fn stop(&self) -> Result<(), BootstrapError> {
        self.0.wake().map_err(BootstrapError::from)
    }
}
