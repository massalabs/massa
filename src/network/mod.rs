use failure::{bail, Error};
use log::{debug, error, warn};
use rand::rngs::OsRng;
use std::fs;
use std::mem::drop;
use std::net::SocketAddr;
use std::path::Path;
use std::{thread, time};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};

use crate::config::NetworkConfig;
use crate::crypto::*;

mod channels;
use channels::CommunicationChannel;

#[derive(Debug)]
pub struct NetworkCommand {
    pub msg: String,
}

#[derive(Debug)]
enum ConnectionEvent {
    ConnectionFailed { address: SocketAddr },
    ChannelCandidate { channel: CommunicationChannel },
}

async fn channel_establish_process(
    network_config: NetworkConfig,
    secret_key: SecretKey,
    public_key: PublicKey,
    mut conn_event_tx: mpsc::Sender<ConnectionEvent>,
    socket: TcpStream,
    remote_addr: SocketAddr,
    shutdown_rx_copy: watch::Receiver<bool>,
) {
    match channels::establish_channel(&network_config, &secret_key, &public_key, socket).await {
        Ok(chan) => {
            // send channel candidate
            match conn_event_tx
                .send(ConnectionEvent::ChannelCandidate { channel: chan })
                .await
            {
                Ok(_) => {}
                Err(e_send) => {
                    error!(
                        "Couldn't send channel creation message: {:?} ; remote: {:?}",
                        e_send, &remote_addr
                    );
                }
            }
        }
        Err(e_establish) => {
            // send error
            debug!(
                "Couldn't establish channel: {:?} ; remote: {:?}",
                e_establish, &remote_addr
            );
            match conn_event_tx
                .send(ConnectionEvent::ConnectionFailed {
                    address: remote_addr.clone(),
                })
                .await
            {
                Ok(_) => {}
                Err(e_send) => {
                    error!(
                        "Couldn't send channel creation error message: {:?} ; remote: {:?}",
                        e_send, &remote_addr
                    );
                }
            }
        }
    }
    drop(shutdown_rx_copy);
}

async fn listener_process(
    network_config: NetworkConfig,
    secret_key: SecretKey,
    public_key: PublicKey,
    conn_event_tx: mpsc::Sender<ConnectionEvent>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<(), Error> {
    // launch TCP listener ; wait and retry on error
    let mut tcp_listener;
    loop {
        tokio::select! {
            listen_result = TcpListener::bind(&network_config.bind) => {
                match listen_result {
                    Ok(listener) => {
                        tcp_listener = listener;
                        break;
                    },
                    Err(e) => {
                        warn!("Couldn't bind TCP listener (will retry after delay): {:?}", e);
                        thread::sleep(time::Duration::from_secs_f32(network_config.retry_wait));
                        continue;
                    },
                };
            },
            opt_stop = shutdown_rx.recv() => { match opt_stop {
                Some(false) => {},
                _ => {
                    return Ok(());
                }
            };},
        };
    }
    debug!("Listening on {}", &network_config.bind);

    // accept incoming TCP connections
    loop {
        tokio::select! {
            accept_result = tcp_listener.accept() => { match accept_result {
                Ok((socket, remote_addr)) => {
                    let network_config_clone = network_config.clone();
                    let secret_key_clone = secret_key.clone();
                    let public_key_clone = public_key.clone();
                    let conn_event_tx_clone = conn_event_tx.clone();
                    let shutdown_rx_copy = shutdown_rx.clone();
                    let remote_addr_copy = remote_addr.clone();
                    tokio::spawn(async move {
                        channel_establish_process(
                            network_config_clone,
                            secret_key_clone,
                            public_key_clone,
                            conn_event_tx_clone,
                            socket,
                            remote_addr_copy,
                            shutdown_rx_copy
                        ).await;
                    });
                },
                Err(e_listen) => {
                    debug!("Couldn't accept incoming TCP connection: {:?}", e_listen);
                    continue;
                }
            };},
            opt_stop = shutdown_rx.recv() => { match opt_stop {
                Some(false) => {},
                _ => {
                    return Ok(());
                }
            };},
        };
    }
}

fn load_or_generate_node_identity(node_key_file: &String) -> Result<(SecretKey, PublicKey), Error> {
    // load node auth key from file if exists. Otherwise generate a new one and write it to file
    let secret_key;
    if Path::new(node_key_file).exists() {
        let b58check_privkey = fs::read_to_string(node_key_file)?;
        secret_key = SecretKey::from_b58check(&b58check_privkey)?;
    } else {
        let mut rng = OsRng::default();
        secret_key = SecretKey::random(&mut rng);
        fs::write(node_key_file, secret_key.into_b58check())?;
    }
    let public_key = PublicKey::from_secret_key(&secret_key);
    Ok((secret_key, public_key))
}

pub async fn run(
    network_config: &NetworkConfig,
    mut network_command_rx: mpsc::Receiver<NetworkCommand>,
) -> Result<(), Error> {
    // get node identity
    let (secret_key, public_key) = load_or_generate_node_identity(&network_config.node_key_file)?;

    // load trusted nodes
    // TODO

    // load known nodes
    // TODO

    // setup shutdown watch
    let (mut shutdown_tx, shutdown_rx) = watch::channel(false);

    // setup connection event receiver
    const CONN_EVENT_MPSC_CAPACITY: usize = 128;
    let (conn_event_tx, mut conn_event_rx) = mpsc::channel(CONN_EVENT_MPSC_CAPACITY);

    // setup listener process
    {
        let network_config_clone = network_config.clone();
        let secret_key_clone = secret_key.clone();
        let public_key_clone = public_key.clone();
        let conn_event_tx_clone = conn_event_tx.clone();
        let shutdown_rx_clone = shutdown_rx.clone();
        tokio::spawn(async move {
            listener_process(
                network_config_clone,
                secret_key_clone,
                public_key_clone,
                conn_event_tx_clone,
                shutdown_rx_clone,
            )
            .await
        });
    }

    // wait for incoming messages and events
    loop {
        tokio::select! {
            opt_msg = network_command_rx.recv() => { match opt_msg {
                Some(msg) => {
                    warn!("Received message: {}", msg.msg);
                    // TODO run message dispatcher function
                },
                None => {
                    // all command senders have been dropped: network is not in use anymore
                    // Warning: if we add other channels (e.g. broadcast), check that they are dropped as well
                    break;
                }
            };},
            opt_conn_evt = conn_event_rx.recv() => { match opt_conn_evt {
                Some(ConnectionEvent::ConnectionFailed{address}) => {
                    // TODO look at "address"
                },
                Some(ConnectionEvent::ChannelCandidate{channel}) => {
                    // TODO look at "channel"
                },
                None => {}
            };},
        };
    }

    // shutdown
    debug!("Shutting down network module");
    drop(shutdown_rx);
    match shutdown_tx.broadcast(true) {
        Ok(_) => {}
        Err(e) => {
            bail!("Error while broadcasting shutdown message: {:?}", e);
        }
    }
    network_command_rx.close();
    conn_event_rx.close();
    shutdown_tx.closed().await;

    // everything went fine
    debug!("Network module cleanly shut down");
    Ok(())
}
