use std::net::{SocketAddr, TcpListener, TcpStream};

use mio::net::TcpListener as MioTcpListener;

use mio::{Events, Interest, Poll, Token, Waker};
use tracing::{error, info};

use crate::error::BootstrapError;

const NEW_CONNECTION: Token = Token(0);
const STOP_LISTENER: Token = Token(10);

pub(crate) struct BootstrapTcpListener();

pub struct BootstrapListenerStopHandle(Waker);

impl BootstrapTcpListener {
    /// Start a new bootstrap listener on the given address.
    ///
    /// * `addr` - the address to listen on
    /// * `connection_tx` - the channel to send new connections to
    pub fn start(
        addr: SocketAddr,
        connection_tx: crossbeam::channel::Sender<(TcpStream, SocketAddr)>,
    ) -> Result<BootstrapListenerStopHandle, BootstrapError> {
        info!("Starting bootstrap listener on {}", &addr);
        let server = TcpListener::bind(addr)?;
        let mut mio_server =
            MioTcpListener::from_std(server.try_clone().expect("Unable to clone server socket"));

        let mut poll = Poll::new()?;

        // wake up the poll when we want to stop the listener
        let waker = mio::Waker::new(poll.registry(), STOP_LISTENER)?;

        poll.registry()
            .register(&mut mio_server, NEW_CONNECTION, Interest::READABLE)?;

        // TODO use config for capacity ?
        let mut events = Events::with_capacity(32);

        // spawn a new thread to handle events
        std::thread::Builder::new()
            .name("bs_listener".to_string())
            .spawn(move || loop {
                poll.poll(&mut events, None).unwrap();
                info!("polling events:");

                for event in events.iter() {
                    match event.token() {
                        NEW_CONNECTION => {
                            println!("New connection: {}", addr);
                            // Here we need std::TcpStream
                            let (socket, addr) =
                                server.accept().map_err(BootstrapError::from).unwrap();
                            println!("send msg: {}", addr);
                            connection_tx.send((socket, addr)).unwrap();
                        }
                        STOP_LISTENER => {
                            info!("Stopping bootstrap listener");
                            return;
                        }
                        _ => error!("Unexpected event"),
                    }
                }
            })
            .expect("in `start bootstrap listener`, OS failed to spawn listener thread");

        Ok(BootstrapListenerStopHandle(waker))
    }
}

impl BootstrapListenerStopHandle {
    /// Stop the bootstrap listener.
    pub fn stop(self) -> Result<(), BootstrapError> {
        self.0.wake().map_err(BootstrapError::from)
    }
}
