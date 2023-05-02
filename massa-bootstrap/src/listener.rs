use std::net::{SocketAddr, TcpListener, TcpStream};

use mio::net::TcpListener as MioTcpListener;

use mio::{Events, Interest, Poll, Token, Waker};
use tracing::info;

use crate::error::BootstrapError;
use crate::establisher::BSEventPoller;

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
    /// Start a new bootstrap listener on the given address.
    ///
    /// * `addr` - the address to listen on
    /// * `connection_tx` - the channel to send new connections to
    pub fn new(addr: SocketAddr) -> Result<(BootstrapListenerStopHandle, Self), BootstrapError> {
        info!("Starting bootstrap listener on {}", &addr);
        let server = TcpListener::bind(addr)?;
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
    fn poll(&mut self) -> Result<PollEvent, BootstrapError> {
        self.poll.poll(&mut self.events, None).unwrap();

        // Confirm that we are not being signalled to shut down
        if self.events.iter().any(|ev| ev.token() == STOP_LISTENER) {
            return Ok(PollEvent::Stop);
        }

        // Ther could be more than one connection ready, but we want to re-check for the stop
        // signal after processing each connection.
        Ok(PollEvent::NewConnection(
            self.server.accept().map_err(BootstrapError::from)?,
        ))
    }
}

impl BootstrapListenerStopHandle {
    /// Stop the bootstrap listener.
    pub fn stop(&self) -> Result<(), BootstrapError> {
        self.0.wake().map_err(BootstrapError::from)
    }
}
