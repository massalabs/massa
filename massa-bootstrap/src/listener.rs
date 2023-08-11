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

pub struct BootstrapListenerStopHandle(Waker);

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
        info!("Starting bootstrap listener on {}", &addr);

        let mut server = mio::net::TcpListener::bind(*addr)?;

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

    pub(crate) fn poll(&mut self) -> Result<PollEvent, BootstrapError> {
        self.poll.poll(&mut self.events, None).unwrap();

        let mut results = Vec::with_capacity(self.events.iter().count());

        // Process each event.
        for event in self.events.iter() {
            match event.token() {
                NEW_CONNECTION => loop {
                    match self.server.accept() {
                        Ok((mut stream, remote_addr)) => {
                            let _ = self.poll.registry().deregister(&mut stream);
                            let stream: std::net::TcpStream = mio_stream_to_std(stream);
                            stream.set_nonblocking(false)?;
                            results.push((stream, remote_addr));
                        }
                        Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                            break;
                        }
                        Err(e) => {
                            warn!("Error accepting connection in bootstrap: {:?}", e);
                            continue;
                        }
                    }
                },
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
