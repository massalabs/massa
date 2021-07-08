use failure::{bail, Error};
use rand::rngs::OsRng;
use rand::Rng;
use tokio::net::TcpStream;
use tokio::prelude::*;
use tokio::time::{timeout, Duration};
use tokio::sync::{mpsc, watch};
use std::net::{SocketAddr, Shutdown};
use log::{debug, error, warn};


use crate::config::NetworkConfig;
use crate::crypto::*;

#[derive(Debug)]
pub enum Message {
    ChannelAccepted,
}

#[derive(Debug)]
pub struct Channel {
    pub address: SocketAddr,
    pub public_key: PublicKey,
    pub sender: mpsc::Sender<Message>,
}

#[derive(Debug)]
pub enum ChannelEvent {
    Candidate { channel: Channel },
}


async fn perform_handshake(
    network_config: &NetworkConfig,
    secret_key: &SecretKey,
    public_key: &PublicKey,
    socket: &mut TcpStream,
) -> Result<PublicKey, Error> {
    const HANDSHAKE_RANDOM_BYTES: usize = 32;
    // split socket for full duplex operation
    let (mut reader, mut writer) = socket.split();

    // prepare local handshake message (randomnes + pubkey) + signature
    let local_randomnes;
    let local_handshake_data;
    {
        let mut local_randomnes_mut = [0u8; HANDSHAKE_RANDOM_BYTES];
        let mut rng = OsRng::default();
        rng.try_fill(&mut local_randomnes_mut)?;
        local_randomnes = local_randomnes_mut;
        let local_pubkey = public_key.serialize_compressed();
        let data_to_sign = [&local_randomnes[..], &local_pubkey[..]].concat();
        let signature = secret_key.generate_signature(&data_to_sign)?;
        local_handshake_data = [&data_to_sign[..], &signature[..]].concat();
    }

    // send local handshake and read remote handshake
    let mut remote_handshake_data =
        [0u8; HANDSHAKE_RANDOM_BYTES + COMPRESSED_PUBLIC_KEY_SIZE + SIGNATURE_SIZE];
    {
        let (res1, res2) = tokio::try_join!(
            timeout(
                Duration::from_secs_f32(network_config.timeout),
                writer.write_all(&local_handshake_data)
            ),
            timeout(
                Duration::from_secs_f32(network_config.timeout),
                reader.read_exact(&mut remote_handshake_data)
            )
        )?;
        res1?;
        res2?;
        //TODO stop when one fails (not just when it times out)
    }

    // parse and verify remote handshake, prepare local response
    let remote_pubkey;
    let local_repsonse_data;
    {
        let mut remote_randomnes = [0u8; HANDSHAKE_RANDOM_BYTES];
        remote_randomnes.copy_from_slice(&remote_handshake_data[..HANDSHAKE_RANDOM_BYTES]);
        remote_pubkey = PublicKey::parse_slice(
            &remote_handshake_data
                [(HANDSHAKE_RANDOM_BYTES)..(HANDSHAKE_RANDOM_BYTES + COMPRESSED_PUBLIC_KEY_SIZE)],
            Some(PublicKeyFormat::Compressed),
        )?;
        if remote_pubkey == *public_key {
            bail!("Remote public key same as local public key");
        }
        let mut sig = [0u8; SIGNATURE_SIZE];
        sig.copy_from_slice(
            &remote_handshake_data[(HANDSHAKE_RANDOM_BYTES + COMPRESSED_PUBLIC_KEY_SIZE)..],
        );
        remote_pubkey.verify_signature(
            &remote_handshake_data[..(HANDSHAKE_RANDOM_BYTES + COMPRESSED_PUBLIC_KEY_SIZE)],
            &sig,
        )?;
        local_repsonse_data = secret_key.generate_signature(&remote_randomnes)?;
    }

    // send local response and read remote response
    let mut remote_response_data = [0u8; SIGNATURE_SIZE];
    {
        let (res1, res2) = tokio::try_join!(
            timeout(
                Duration::from_secs_f32(network_config.timeout),
                writer.write_all(&local_repsonse_data)
            ),
            timeout(
                Duration::from_secs_f32(network_config.timeout),
                reader.read_exact(&mut remote_response_data)
            )
        )?;
        res1?;
        res2?;
        //TODO stop when one fails (not just when it times out)
    }

    // check their signature of our randomnes
    remote_pubkey.verify_signature(&local_randomnes, &remote_response_data)?;
    
    // return remote public key
    Ok(remote_pubkey)
}

async fn channel_event_loop(
    network_config: &NetworkConfig,
    secret_key: &SecretKey,
    public_key: &PublicKey,
    socket: &mut TcpStream,
    channel_rx: &mut mpsc::Receiver<Message>,
    channel_event_tx: &mut mpsc::Sender<ChannelEvent>,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> Result<(), Error> {
    
    Ok(())
}

async fn channel_process(
    network_config: &NetworkConfig,
    secret_key: &SecretKey,
    public_key: &PublicKey,
    mut socket: TcpStream,
    mut channel_event_tx: mpsc::Sender<ChannelEvent>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<(), Error> {
    // get remote address
    let remote_address = socket.peer_addr()?;

    // perform handshake and get remote public key
    let remote_public_key = perform_handshake(
        &network_config,
        &secret_key,
        &public_key,
        &mut socket
    ).await?;

    // create sender link towards network manager and candidate
    let mut channel_rx;
    {
        const CHANNEL_MPSC_CAPACITY: usize = 128;
        let (chan_tx, chan_rx) = mpsc::channel(CHANNEL_MPSC_CAPACITY);
        channel_event_tx.send(ChannelEvent::Candidate {
            channel: Channel {
                address: remote_address.clone(),
                public_key: remote_public_key.clone(),
                sender: chan_tx,
            }
        }).await?;
        channel_rx = chan_rx;
    }
    // read network manager response
    match channel_rx.recv().await {
        Some(Message::ChannelAccepted) => {},
        None => {  // channel refused
            // TODO try to send packet saying that the channel was refused
            bail!("Channel refused by network manager");
        },
    };
    
    // run channel event loop
    match channel_event_loop(
        &network_config,
        &secret_key,
        &public_key,
        &mut socket,
        &mut channel_rx,
        &mut channel_event_tx,
        &mut shutdown_rx,
    ).await {
        Ok(_) => {},
        Err(e) => {
            debug!("Channel closed on error: {:?}", e);
        },
    }

    // close link from network manager
    channel_rx.close();

    // try to notify network manager of closure
    /* TODO
        match channel_event_tx.send().await {
            Ok(_) => {},
            Err(e) => {
                warn!("Could not notify manager of channel closure: {:?}", e);
            },
        }
    */
    
    // try to gracefully shutdown socket
    match socket.shutdown(Shutdown::Both) {
        Ok(_) => {},
        Err(e) => {
            debug!("Could not cleanly shutdown socket: {:?}", e);
        },
    };

    Ok(())
}

pub async fn launch_detached(
    network_config: &NetworkConfig,
    secret_key: &SecretKey,
    public_key: &PublicKey,
    socket: TcpStream,
    channel_event_tx: &mut mpsc::Sender<ChannelEvent>,
    shutdown_rx: &mut watch::Receiver<bool>,
) {
    let network_config_clone = network_config.clone();
    let secret_key_clone = secret_key.clone();
    let public_key_clone = public_key.clone();
    let channel_event_tx_clone = channel_event_tx.clone();
    let shutdown_rx_clone = shutdown_rx.clone();
    tokio::spawn(async move {
        // TODO notify main of a new connection to this IP
        let _ = channel_process(
            &network_config_clone,
            &secret_key_clone,
            &public_key_clone,
            socket,
            channel_event_tx_clone,
            shutdown_rx_clone,
        ).await;
        // TODO notify main of a closed connection to this IP
    });
}
