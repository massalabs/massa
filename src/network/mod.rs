use failure::{bail, Error};
use log::{debug, error, warn};
use rand::rngs::OsRng;
use std::fs;
use std::net::SocketAddr;
use std::path::Path;
use std::{thread, time};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};

use crate::config::NetworkConfig;
use crate::crypto::*;

mod channel;

#[derive(Debug)]
pub struct NetworkCommand {
    pub msg: String,
}


async fn listener_process(
    network_config: NetworkConfig,
    secret_key: SecretKey,
    public_key: PublicKey,
    mut chan_event_tx: mpsc::Sender<channel::ChannelEvent>,
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
                Ok((socket, _)) => {
                    channel::launch_detached(
                        &network_config,
                        &secret_key,
                        &public_key,
                        socket,
                        &mut chan_event_tx,
                        &mut shutdown_rx
                    ).await;
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
    let (chan_event_tx, mut chan_event_rx) = mpsc::channel(CONN_EVENT_MPSC_CAPACITY);

    // setup listener process
    {
        let network_config_clone = network_config.clone();
        let secret_key_clone = secret_key.clone();
        let public_key_clone = public_key.clone();
        let chan_event_tx_clone = chan_event_tx.clone();
        let shutdown_rx_clone = shutdown_rx.clone();
        tokio::spawn(async move {
            listener_process(
                network_config_clone,
                secret_key_clone,
                public_key_clone,
                chan_event_tx_clone,
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
                    break;
                },
            };},
            opt_chan_evt = chan_event_rx.recv() => { match opt_chan_evt {
                Some(channel::ChannelEvent::Candidate{channel: mut chan}) => {
                    match chan.sender.send(channel::Message::ChannelAccepted).await {
                        Ok(_) => { /* TODO ADD chan TO POOL */ },
                        Err(e) => {
                            warn!("Error notifying channel process of acceptation: {:?}", e);
                        },
                    };
                },
                None => {
                    break;
                },
            };},
        };
    }

    // shutdown
    debug!("Shutting down network module");
    match shutdown_tx.broadcast(true) {
        Ok(_) => {}
        Err(e) => {
            bail!("Error while broadcasting shutdown message: {:?}", e);
        }
    }
    network_command_rx.close();
    chan_event_rx.close();
    shutdown_tx.closed().await;

    // everything went fine
    debug!("Network module cleanly shut down");
    Ok(())
}
