use std::error::Error;
type BoxResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

use super::peer_info_database::*;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};

use super::config::NetworkConfig;

#[derive(Default, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct ConnectionId(u64);

#[derive(Clone, Copy, Debug)]
pub enum ConnectionClosureReason {
    Normal,
    Failed,
    Banned,
}

#[derive(Debug)]
pub enum UpstreamCommand {
    MergeAdvertisedPeerList(Vec<IpAddr>),
    GetAdvertisablePeerList(oneshot::Sender<Vec<IpAddr>>),
    ConnectionClosed((ConnectionId, ConnectionClosureReason)),
    ConnectionAlive(ConnectionId),
}

pub struct ConnectionController {
    upstream_command_tx: mpsc::Sender<UpstreamCommand>,
    event_rx: mpsc::Receiver<ConnectionControllerEvent>,
    controller_fn_handle: JoinHandle<()>,
}

#[derive(Debug)]
pub enum ConnectionControllerEvent {
    NewConnection((ConnectionId, TcpStream)),
    ConnectionBanned(ConnectionId),
}

impl ConnectionController {
    pub async fn new(cfg: &NetworkConfig) -> BoxResult<Self> {
        // check that local IP is routable
        if let Some(our_ip) = cfg.routable_ip {
            if !our_ip.is_global() {
                panic!("config routable_ip IP is not routable");
            }
        }

        // create listener
        let listener = TcpListener::bind(&cfg.bind).await?;

        // load peer info database
        let peer_info_db = PeerInfoDatabase::new(&cfg).await?;

        // launch controller
        let (upstream_command_tx, upstream_command_rx) = mpsc::channel::<UpstreamCommand>(1024);
        let (event_tx, event_rx) = mpsc::channel::<ConnectionControllerEvent>(1024);
        let cfg_copy = cfg.clone();
        let controller_fn_handle = tokio::spawn(async move {
            controller_fn(
                cfg_copy,
                listener,
                peer_info_db,
                upstream_command_rx,
                event_tx,
            )
            .await;
        });

        Ok(ConnectionController {
            upstream_command_tx,
            event_rx,
            controller_fn_handle,
        })
    }

    pub async fn stop(mut self) {
        drop(self.upstream_command_tx);
        self.controller_fn_handle
            .await
            .expect("failed joining network controller");
    }

    pub async fn wait_event(&mut self) -> ConnectionControllerEvent {
        self.event_rx
            .recv()
            .await
            .expect("failed retrieving network controller event")
    }

    pub fn get_upstream_interface(&self) -> ConnectionControllerUpstreamInterface {
        ConnectionControllerUpstreamInterface(self.upstream_command_tx.clone())
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionControllerUpstreamInterface(mpsc::Sender<UpstreamCommand>);

impl ConnectionControllerUpstreamInterface {
    pub async fn merge_advertised_peer_list(&mut self, ips: Vec<IpAddr>) {
        self.0
            .send(UpstreamCommand::MergeAdvertisedPeerList(ips))
            .await
            .expect("network controller disappeared");
    }

    pub async fn get_advertisable_peer_list(&mut self) -> Vec<IpAddr> {
        let (response_tx, response_rx) = oneshot::channel::<Vec<IpAddr>>();
        self.0
            .send(UpstreamCommand::GetAdvertisablePeerList(response_tx))
            .await
            .expect("network controller disappeared");
        response_rx.await.expect("network controller disappeared")
    }

    pub async fn connection_closed(&mut self, id: ConnectionId, reason: ConnectionClosureReason) {
        self.0
            .send(UpstreamCommand::ConnectionClosed((id, reason)))
            .await
            .expect("network controller disappeared");
    }

    pub async fn connection_alive(&mut self, id: ConnectionId) {
        self.0
            .send(UpstreamCommand::ConnectionAlive(id))
            .await
            .expect("network controller disappeared");
    }
}

async fn controller_fn(
    cfg: NetworkConfig,
    listener: TcpListener,
    mut peer_info_db: PeerInfoDatabase,
    mut upstream_command_rx: mpsc::Receiver<UpstreamCommand>,
    event_tx: mpsc::Sender<ConnectionControllerEvent>,
) {
    let mut out_connecting_futures = FuturesUnordered::new();
    let mut cur_connection_id = ConnectionId::default();
    let mut active_connections: HashMap<ConnectionId, (IpAddr, bool)> = HashMap::new(); // ip, is_outgoing

    loop {
        {
            // try to connect to candidate IPs
            let candidate_ips = peer_info_db.get_out_connection_candidate_ips();
            for ip in candidate_ips {
                peer_info_db.new_out_connection_attempt(&ip);
                out_connecting_futures.push(out_connector_fn(
                    SocketAddr::new(ip, cfg.protocol_port),
                    Duration::from_secs_f32(cfg.connect_timeout_seconds),
                ));
            }
        }

        tokio::select! {
            // peer feedback event
            res = upstream_command_rx.next() => match res {
                Some(UpstreamCommand::MergeAdvertisedPeerList(ips)) => {
                    peer_info_db.merge_candidate_peers(&ips);
                },
                Some(UpstreamCommand::GetAdvertisablePeerList(response_tx)) => {
                    response_tx.send(
                        peer_info_db.get_advertisable_peer_ips()
                    ).expect("upstream disappeared");
                },
                Some(UpstreamCommand::ConnectionClosed((id, reason))) => {
                    let (ip, is_outgoing) = active_connections.remove(&id).expect("missing connection closed");
                    match reason {
                        ConnectionClosureReason::Normal => {},
                        ConnectionClosureReason::Failed => { peer_info_db.peer_failed(&ip); },
                        ConnectionClosureReason::Banned =>  {
                            peer_info_db.peer_banned(&ip);
                            // notify all other banned connections to close
                            // note: they must close using Normal reason or else risk of feedback loop
                            let target_ids = active_connections.iter().filter_map(|(other_id, (other_ip, _))| {
                                if *other_ip == ip {
                                    Some(other_id)
                                } else {
                                    None
                                }
                            });
                            for target_id in target_ids {
                                event_tx
                                    .send(ConnectionControllerEvent::ConnectionBanned(*target_id))
                                    .await.expect("could not send connection banned notification upstream");
                            }
                        }
                    }
                    if is_outgoing {
                        peer_info_db.out_connection_closed(&ip);
                    } else {
                        peer_info_db.in_connection_closed(&ip);
                    }
                },
                Some(UpstreamCommand::ConnectionAlive(id) ) => {
                    let (ip, _) = active_connections.get(&id).expect("missing connection alive");
                    peer_info_db.peer_alive(&ip);
                }
                None => break,
            },

            // out-connector event
            Some((ip_addr, res)) = out_connecting_futures.next() => match res {
                Ok(socket) => if peer_info_db.try_out_connection_attempt_success(&ip_addr) {  // outgoing connection established
                    let connection_id = cur_connection_id;
                    cur_connection_id.0 += 1;
                    active_connections.insert(connection_id, (ip_addr, true));
                    event_tx
                        .send(ConnectionControllerEvent::NewConnection((connection_id, socket)))
                        .await.expect("could not send new out connection notification");
                },
                Err(_) => {
                    peer_info_db.out_connection_attempt_failed(&ip_addr);
                }
            },

            // listener socket received
            res = listener.accept() => match res {
                Ok((socket, remote_addr)) => if peer_info_db.try_new_in_connection(&remote_addr.ip()) {
                    let connection_id = cur_connection_id;
                    cur_connection_id.0 += 1;
                    active_connections.insert(connection_id, (remote_addr.ip(), false));
                    event_tx
                        .send(ConnectionControllerEvent::NewConnection((connection_id, socket)))
                        .await.expect("could not send new in connection notification");
                },
                Err(_) => {},
            }
        }
    }

    // wait for out-connectors to finish
    while let Some(_) = out_connecting_futures.next().await {}

    // stop peer info db
    peer_info_db.stop().await;
}

pub async fn out_connector_fn(
    addr: SocketAddr,
    timeout_duration: Duration,
) -> (IpAddr, Result<TcpStream, String>) {
    match timeout(timeout_duration, tokio::net::TcpStream::connect(addr)).await {
        Ok(Ok(sock)) => (addr.ip(), Ok(sock)),
        Ok(Err(e)) => (addr.ip(), Err(format!("TCP connection failed: {:?}", e))),
        Err(_) => (addr.ip(), Err(format!("TCP connection timed out"))),
    }
}
