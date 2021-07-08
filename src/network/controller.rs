use std::error::Error;
type BoxResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

use super::peer_database::*;
use chrono::Utc;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use log::warn;
use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::{delay_for, Duration};

use super::config::NetworkConfig;

pub struct NetworkController {
    stop_tx: oneshot::Sender<()>,
    peer_feedback_tx: mpsc::Sender<PeerFeedbackEvent>,
    event_rx: mpsc::Receiver<NetworkControllerEvent>,
    controller_fn_handle: JoinHandle<()>,
}

pub enum NetworkControllerEvent {
    CandidateConnection {
        ip: IpAddr,
        socket: TcpStream,
        is_outgoing: bool,
    },
}

pub enum PeerClosureReason {
    Normal,
    ConnectionFailed,
    Banned,
}

pub enum PeerFeedbackEvent {
    PeerList {
        ips: HashSet<IpAddr>,
    },
    PeerClosed {
        ip: IpAddr,
        reason: PeerClosureReason,
    },
    PeerAlive {
        ip: IpAddr,
    },
}

impl NetworkController {
    pub async fn new(cfg: NetworkConfig) -> BoxResult<Self> {
        let peer_db = PeerDatabase::load(cfg.known_peers_file.clone()).await?;

        // launch controller
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        let (peer_feedback_tx, peer_feedback_rx) = mpsc::channel::<PeerFeedbackEvent>(1024);
        let (event_tx, event_rx) = mpsc::channel::<NetworkControllerEvent>(1024);
        let controller_fn_handle = tokio::spawn(async move {
            controller_fn(cfg, peer_db, stop_rx, peer_feedback_rx, event_tx).await;
        });

        Ok(NetworkController {
            stop_tx,
            peer_feedback_tx,
            event_rx,
            controller_fn_handle,
        })
    }

    pub async fn stop(self) -> BoxResult<()> {
        let _ = self.stop_tx.send(());
        self.controller_fn_handle.await?;
        Ok(())
    }

    pub async fn wait_event(&mut self) -> BoxResult<NetworkControllerEvent> {
        self.event_rx
            .recv()
            .await
            .ok_or("error reading events".into())
    }

    pub async fn feedback_peer_list(
        &mut self,
        ips: HashSet<IpAddr>,
    ) -> Result<(), mpsc::error::SendError<PeerFeedbackEvent>> {
        self.peer_feedback_tx
            .send(PeerFeedbackEvent::PeerList { ips })
            .await?;
        Ok(())
    }

    pub async fn feedback_peer_closed(
        &mut self,
        ip: IpAddr,
        reason: PeerClosureReason,
    ) -> Result<(), mpsc::error::SendError<PeerFeedbackEvent>> {
        self.peer_feedback_tx
            .send(PeerFeedbackEvent::PeerClosed { ip, reason })
            .await?;
        Ok(())
    }

    pub async fn feedback_peer_alive(
        &mut self,
        ip: IpAddr,
    ) -> Result<(), mpsc::error::SendError<PeerFeedbackEvent>> {
        self.peer_feedback_tx
            .send(PeerFeedbackEvent::PeerAlive { ip })
            .await?;
        Ok(())
    }
}

async fn controller_fn(
    cfg: NetworkConfig,
    mut peer_db: PeerDatabase,
    mut stop_rx: oneshot::Receiver<()>,
    mut peer_feedback_rx: mpsc::Receiver<PeerFeedbackEvent>,
    mut event_tx: mpsc::Sender<NetworkControllerEvent>,
) {
    let listen_addr = cfg.bind;

    // prepare connectors
    let mut connectors = FuturesUnordered::new();

    // launch listener
    let (listener_stop_tx, listener_stop_rx) = oneshot::channel::<()>();
    let (listener_socket_tx, mut listener_socket_rx) = mpsc::channel::<(IpAddr, TcpStream)>(1024);
    let listener_handle = tokio::spawn(async move {
        listener_fn(listen_addr, listener_stop_rx, listener_socket_tx).await;
    });

    loop {
        peer_db.cleanup(cfg.max_known_nodes); // removes dead connections
        peer_db.save();

        {
            // try to connect to candidate IPs
            let connector_candidate_ips = peer_db.get_connector_candidate_ips();
            for ip in connector_candidate_ips {
                peer_db
                    .peers
                    .get_mut(&ip)
                    .expect("trying to connect to an unkonwn peer")
                    .status = PeerStatus::OutConnecting;
                connectors.push(connector_fn(SocketAddr::new(ip, listen_addr.port())));
            }
        }

        tokio::select! {
            // peer feedback event
            res = peer_feedback_rx.next() => match res {
                Some(PeerFeedbackEvent::PeerList{ips}) => {
                    for ip in ips {
                        peer_db.peers.entry(ip).or_insert(PeerInfo {
                            ip: ip,
                            status:  PeerStatus::Idle,
                            last_connection: None,
                            last_failure: None
                        });
                    }
                },
                Some(PeerFeedbackEvent::PeerClosed{ip, reason}) => {
                    let mut peer = peer_db.peers.get_mut(&ip).expect("disconnected from an unkonwn peer");
                    match reason {
                        PeerClosureReason::Normal => {
                            peer.status = PeerStatus::Idle;
                            peer.last_connection = Some(Utc::now());
                        },
                        PeerClosureReason::ConnectionFailed => {
                            peer.status = PeerStatus::Idle;
                            peer.last_failure = Some(Utc::now());
                        },
                        PeerClosureReason::Banned => {
                            peer.status = PeerStatus::Banned;
                            peer.last_failure = Some(Utc::now());
                        }
                    }
                },
                Some(PeerFeedbackEvent::PeerAlive { ip } ) => {
                    let mut peer = peer_db.peers.get_mut(&ip).expect("conection OK from an unkonwn peer");
                    peer.status = match peer.status {
                        PeerStatus::InHandshaking => PeerStatus::InConnected,
                        PeerStatus::OutHandshaking => PeerStatus::OutConnected,
                        _ => unreachable!("connection OK from peer that was not in the process of connecting")
                    };
                    peer.last_connection = Some(Utc::now());
                }
                None => unreachable!("peer feedback channel disappeared"),
            },

            // connector event
            Some((ip_addr, res)) = connectors.next() => {
                let peer = match peer_db.peers.get_mut(&ip_addr) {
                    Some(p) => match p.status {
                        PeerStatus::OutConnecting => p,
                        _ => continue  // not in OutConnecting status (avoid double-connection)
                    },
                    _ => continue  // not in known peer list
                };
                match res {
                    Ok(socket) => {
                        peer.status = PeerStatus::OutHandshaking;
                        if event_tx.send(NetworkControllerEvent::CandidateConnection{
                            ip: ip_addr,
                            socket: socket,
                            is_outgoing: true
                        }).await.is_err() { unreachable!("could not send out-connected peer upstream") }
                    },
                    Err(_) => {
                        peer.status = PeerStatus::Idle;
                        peer.last_failure = Some(Utc::now());
                    },
                }
            },

            // listener event
            res = listener_socket_rx.next() => match res {
                Some((ip_addr, socket)) => {
                    if peer_db.count_peers_with_status(PeerStatus::InHandshaking) >= cfg.max_simultaneous_incoming_connection_attempts { continue }
                    if peer_db.count_peers_with_status(PeerStatus::InConnected) >= cfg.max_incoming_connections { continue }
                    let peer = peer_db.peers.entry(ip_addr).or_insert(PeerInfo {
                        ip: ip_addr,
                        status: PeerStatus::Idle,
                        last_connection: None,
                        last_failure: None
                    });
                    match peer.status {
                        PeerStatus::OutConnecting => {}, // override out-connection attempts (but not handshake)
                        PeerStatus::Idle => {},
                        _ => continue, // avoid double-connection and banned
                    }
                    peer.status = PeerStatus::InHandshaking;
                    if event_tx.send(NetworkControllerEvent::CandidateConnection{
                        ip: ip_addr,
                        socket: socket,
                        is_outgoing: false
                    }).await.is_err() { unreachable!("could not send in-connected peer upstream") }
                },
                None => unreachable!("listener disappeared"),
            },

            // stop message
            _ = &mut stop_rx => break,
        }
    }

    // stop listener
    listener_socket_rx.close();
    let _ = listener_stop_tx.send(());
    let _ = listener_handle.await;

    // wait for connectors to finish
    while let Some(_) = connectors.next().await {}

    // stop peer db
    peer_db.cleanup(cfg.max_known_nodes); // removes dead connections
    peer_db.stop().await;
}

pub async fn listener_fn(
    addr: SocketAddr,
    mut stop_rx: oneshot::Receiver<()>,
    mut socket_tx: mpsc::Sender<(IpAddr, TcpStream)>,
) {
    'reset_loop: loop {
        if let Err(oneshot::error::TryRecvError::Empty) = stop_rx.try_recv() {
        } else {
            break 'reset_loop;
        }
        let mut listener = tokio::select! {
            res = TcpListener::bind(addr) => match res {
                Ok(v) => v,
                Err(e) => {
                    warn!("network listener bind error: {}", e);
                    delay_for(Duration::from_secs(1)).await;
                    continue 'reset_loop;
                },
            },
            _ = &mut stop_rx => break 'reset_loop,
        };
        loop {
            tokio::select! {
                res = listener.accept() => match res {
                    Ok((socket, remote_addr)) => {
                        if socket_tx.send((remote_addr.ip(), socket)).await.is_err() {
                            break 'reset_loop;
                        }
                    },
                    Err(_) => {},
                },
                _ = &mut stop_rx => break 'reset_loop,
            }
        }
    }
}

pub async fn connector_fn(addr: SocketAddr) -> (IpAddr, BoxResult<TcpStream>) {
    match tokio::net::TcpStream::connect(addr).await {
        Ok(socket) => (addr.ip(), Ok(socket)),
        Err(e) => (addr.ip(), Err(Box::new(e))),
    }
}
