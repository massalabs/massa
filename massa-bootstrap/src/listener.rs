use std::net::SocketAddr;

use mio::{net::TcpListener, Events, Interest, Poll, Token};
use tracing::{info, log::warn};

use crate::error::BootstrapError;

const NEW_CONNECTION: Token = Token(0);
const STOP_LISTENER: Token = Token(10);

pub(crate) struct BootstrapTcpListener {
    addr: SocketAddr,
    // stop_handle:
}

impl BootstrapTcpListener {
    pub fn new(addr: SocketAddr) -> Self {
        BootstrapTcpListener { addr }
    }

    pub fn run(&mut self) -> Result<(), BootstrapError> {
        let mut server = TcpListener::bind(self.addr)?;
        let mut poll = Poll::new()?;
        poll.registry()
            .register(&mut server, NEW_CONNECTION, Interest::READABLE)?;

        // TODO use config for capacity ?
        let mut events = Events::with_capacity(32);

        loop {
            poll.poll(&mut events, None)?;

            for event in events.iter() {
                match event.token() {
                    NEW_CONNECTION => {
                        let (mut socket, addr) =
                            server.accept().map_err(|e| BootstrapError::from(e))?;
                        println!("New connection: {}", addr);
                        // poll.register(&socket, STOP_LISTENER, Ready::readable(), PollOpt::edge())
                        //     .unwrap();
                    }
                    STOP_LISTENER => {
                        info!("Stopping bootstrap listener");
                        return Ok(());
                    }
                    _ => warn!("Unexpected event"),
                }
            }
        }
    }
}
