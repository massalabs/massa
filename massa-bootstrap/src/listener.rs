use std::net::{SocketAddr, TcpListener, TcpStream};

use mio::net::TcpListener as MioTcpListener;

use mio::{Events, Interest, Poll, Token, Waker};
use tracing::{error, info};

use crate::error::BootstrapError;

const NEW_CONNECTION: Token = Token(0);
const STOP_LISTENER: Token = Token(10);

pub(crate) struct BootstrapTcpListener {
    poll: Poll,
    events: Events,
    server: TcpListener,
    // HACK : create variable to move ownership of mio_server to the thread
    // if mio_server is not moved, poll does not receive any event from listener
    _mio_server: MioTcpListener,
}

pub struct BootstrapListenerStopHandle(Waker);

pub enum AcceptEvent {
    NewConnection((TcpStream, SocketAddr)),
    Stop,
}

impl BootstrapTcpListener {
    /// Start a new bootstrap listener on the given address.
    ///
    /// * `addr` - the address to listen on
    /// * `connection_tx` - the channel to send new connections to
    pub fn new(
        addr: SocketAddr,
    ) -> Result<(BootstrapListenerStopHandle, BootstrapTcpListener), BootstrapError> {
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

    pub(crate) fn accept(&mut self) -> Result<AcceptEvent, BootstrapError> {
        self.poll.poll(&mut self.events, None).unwrap();

        for event in self.events.iter() {
            match event.token() {
                NEW_CONNECTION => {
                    return Ok(AcceptEvent::NewConnection(
                        self.server.accept().map_err(BootstrapError::from)?,
                    ));
                }
                STOP_LISTENER => {
                    info!("Stopping bootstrap listener");
                    return Ok(AcceptEvent::Stop);
                }
                _ => error!("Unexpected event"),
            }
        }
        panic!("TODO: handle No event received");
    }
}

impl BootstrapListenerStopHandle {
    /// Stop the bootstrap listener.
    pub fn stop(self) -> Result<(), BootstrapError> {
        self.0.wake().map_err(BootstrapError::from)
    }
}
