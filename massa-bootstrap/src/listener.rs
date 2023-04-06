use std::net::SocketAddr;

use mio::{
    net::{TcpListener, TcpStream},
    Events, Interest, Poll, Token, Waker,
};
use tracing::{error, info};

use crate::error::BootstrapError;

const NEW_CONNECTION: Token = Token(0);
const STOP_LISTENER: Token = Token(10);

pub(crate) struct BootstrapTcpListener();

pub struct BootstrapListenerStopHandle(Waker);

impl BootstrapTcpListener {
    /// Start a new bootstrap listener on the given address.
    pub fn start(
        addr: SocketAddr,
        connection_tx: crossbeam::channel::Sender<(TcpStream, SocketAddr)>,
    ) -> Result<BootstrapListenerStopHandle, BootstrapError> {
        let mut server = TcpListener::bind(addr)?;
        let mut poll = Poll::new()?;

        // wake up the poll when we want to stop the listener
        let waker = mio::Waker::new(poll.registry(), STOP_LISTENER)?;

        poll.registry()
            .register(&mut server, NEW_CONNECTION, Interest::READABLE)?;

        // TODO use config for capacity ?
        let mut events = Events::with_capacity(32);

        // spawn a new thread to handle events
        std::thread::spawn(move || loop {
            poll.poll(&mut events, None).unwrap();

            for event in events.iter() {
                match event.token() {
                    NEW_CONNECTION => {
                        let (socket, addr) = server
                            .accept()
                            .map_err(|e| BootstrapError::from(e))
                            .unwrap();
                        println!("New connection: {}", addr);
                        connection_tx.send((socket, addr)).unwrap();
                    }
                    STOP_LISTENER => {
                        info!("Stopping bootstrap listener");
                        return;
                    }
                    _ => error!("Unexpected event"),
                }
            }
        });

        Ok(BootstrapListenerStopHandle(waker))
    }
}

impl BootstrapListenerStopHandle {
    pub fn stop(self) {
        self.0.wake().unwrap();
    }
}
