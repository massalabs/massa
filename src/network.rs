use log::{error, warn, info, debug, trace};
use crate::config;
use crate::crypto::*;
use failure::Error;
use rand::Rng;
use rand::rngs::OsRng;
use std::fs;
use std::path::Path;
use tokio::prelude::*;
use std::{thread, time};
use tokio::time::{timeout, Duration};

// TODO add graceful stop

// async fn listen(bind: &str) -> Result<(), Error> {
//     let mut listener = tokio::net::TcpListener::bind(bind).await?;
//     loop {
//         let (mut socket, _) = listener.accept().await?;
//         tokio::spawn(async move {
//             let mut buf = [0; 1024];
//             loop {
//                 // TODO here perform Receive handshake
//                 // ---

//                 let n = match socket.read(&mut buf).await {
//                     // socket closed
//                     Ok(0) => return,
//                     Ok(n) => n,
//                     Err(e) => {
//                         println!("failed to read from socket; err = {:?}", e);
//                         return;
//                     }
//                 };

//                 // Write the data back
//                 if let Err(e) = socket.write_all(&buf[0..n]).await {
//                     println!("failed to write to socket; err = {:?}", e);
//                     return;
//                 }
//             }
//         });
//     }
// }

#[derive(Debug, Clone)]
pub struct NetworkParameters {
    pub public_key: PublicKey,
    pub secret_key: SecretKey,
    pub network_config: config::NetworkConfig,
}

#[derive(Debug)]
pub struct Channel {
}

// async fn network_handler(network_parameters: NetworkParameters) -> Result<(), Error> {

//     // ask for blocks

//     // declare lists

//     {
//         let network_parameters_clone = network_parameters.clone();
//         tokio::spawn(async move {
//             listener(network_parameters_clone).await
//         });
//     }
    
//     Ok(())
// }

async fn perform_handshake<R: Rng>(network_parameters: &NetworkParameters, socket: &mut tokio::net::TcpStream, rng: &mut R) -> Result<PublicKey, Error> {
    const HANDSHAKE_RANDOM_BYTES:usize = 32;
    // split socket for full duplex operation
    let (mut reader, mut writer) = socket.split();
    
    // prepare local handshake message (randomnes + pubkey) + signature
    let local_randomnes;
    let local_handshake_data;
    {
        let mut local_randomnes_mut = [0u8; HANDSHAKE_RANDOM_BYTES];
        rng.try_fill(&mut local_randomnes_mut)?;
        local_randomnes = local_randomnes_mut;
        let local_pubkey = network_parameters.public_key.serialize_compressed();
        let data_to_sign = [&local_randomnes[..], &local_pubkey[..]].concat();
        let signature = network_parameters.secret_key.generate_signature(&data_to_sign)?;
        local_handshake_data = [&data_to_sign[..], &signature[..]].concat();
    }

    // at the same time: send local handshake and read remote handshake
    let mut remote_handshake_data = [0u8; HANDSHAKE_RANDOM_BYTES + COMPRESSED_PUBLIC_KEY_SIZE + SIGNATURE_SIZE];
    {
        let (res1, res2) = tokio::join!(
            timeout(
                Duration::from_secs_f32(network_parameters.network_config.timeout),
                writer.write_all(&local_handshake_data)
            ),
            timeout(
                Duration::from_secs_f32(network_parameters.network_config.timeout),
                reader.read_exact(&mut remote_handshake_data)
            )
        );
        res1??;
        res2??;
        // TODO future improvement : abort as soon as any of the two fails
    }

    // parse and verify remote handshake, prepare local response
    let remote_pubkey;
    let local_repsonse_data;
    {
        let mut remote_randomnes = [0u8 ; HANDSHAKE_RANDOM_BYTES];
        remote_randomnes.copy_from_slice(&remote_handshake_data[..HANDSHAKE_RANDOM_BYTES]);
        remote_pubkey = PublicKey::parse_slice(
            &remote_handshake_data[(HANDSHAKE_RANDOM_BYTES)..(HANDSHAKE_RANDOM_BYTES+COMPRESSED_PUBLIC_KEY_SIZE)],
            Some(PublicKeyFormat::Compressed)
        )?;
        let mut sig = [0u8 ; SIGNATURE_SIZE];
        sig.copy_from_slice(&remote_handshake_data[(HANDSHAKE_RANDOM_BYTES+COMPRESSED_PUBLIC_KEY_SIZE)..]);
        remote_pubkey.verify_signature(
            &remote_handshake_data[..(HANDSHAKE_RANDOM_BYTES+COMPRESSED_PUBLIC_KEY_SIZE)],
            &sig
        )?;
        local_repsonse_data = network_parameters.secret_key.generate_signature(&remote_randomnes)?;
    }

    // at the same time, send local response and read remote response
    let mut remote_response_data = [0u8; SIGNATURE_SIZE];
    {
        let (res1, res2) = tokio::join!(
            timeout(
                Duration::from_secs_f32(network_parameters.network_config.timeout),
                writer.write_all(&local_repsonse_data)
            ),
            timeout(
                Duration::from_secs_f32(network_parameters.network_config.timeout),
                reader.read_exact(&mut remote_response_data)
            )
            // TODO future improvement : abort as soon as any of the two fails
        );
        res1??;
        res2??;
    }

    // wait and check their signature of our randomnes
    remote_pubkey.verify_signature(&local_randomnes, &remote_response_data)?;

    // return remote public key
    Ok(remote_pubkey)
}

async fn establish_channel<R: Rng>(network_parameters: &NetworkParameters, socket: &mut tokio::net::TcpStream, remote_addr: &std::net::SocketAddr, rng: &mut R) -> Result<Channel, Error> {
    // atomically add 1 slot to this IP => quit if too many (must decrement if the function quits before channel establishment !)
    
    // perform handshake
    let remote_public_key = perform_handshake(network_parameters, socket, rng).await?;
    
    // TODO check if trusted 

    // TODO atomically:
    //  check if already exists => exit if so
    //  add to established channels (trusted ?) list

    Ok(Channel {
        // TODO remote_addr
        // TODO socket
        // TODO remote_public_key
    })
}


async fn listener_process(network_parameters: NetworkParameters) -> Result<(), Error> {
    let mut rng = OsRng::default();
    loop {
        // launch TCP listener ; wait and retry on error
        let mut tcp_listener = match tokio::net::TcpListener::bind(&network_parameters.network_config.bind).await {
            Ok(v) => v,
            Err(e) => {
                warn!("Couldn't bind TCP listener (will retry after delay): {:?}", e);
                thread::sleep(time::Duration::from_secs_f32(network_parameters.network_config.retry_wait));
                continue;
            },
        };
        debug!("Listening on {}", &network_parameters.network_config.bind);

        // accept incoming TCP connections and perform incoming handshake
        loop {
            let (mut socket, remote_addr) = match tcp_listener.accept().await {
                Ok(v) => v,
                Err(e) => {
                    debug!("Couldn't accept incoming TCP connection: {:?}", e);
                    continue;
                }
            };
            match establish_channel(&network_parameters, &mut socket, &remote_addr, &mut rng).await {
                Ok(c) => {
                    debug!("Established channel from incoming TCP connection: {:?}", &c)
                },
                Err(e) => {
                    debug!("Could not establish channel with incoming TCP connection with {:?}: {:?}", &remote_addr, e)
                }
            }
        }
    }

    Ok(())
}

pub async fn run(network_config: config::NetworkConfig) -> Result<(), Error> {
    // initialize RNG
    let mut rng = OsRng::default();
    
    // load node auth key from file if exists. Otherwise generate a new one and write it to file
    let secret_key;
    if Path::new(&network_config.node_key_file).exists() {
        let b58check_privkey = fs::read_to_string(&network_config.node_key_file)?;
        secret_key = SecretKey::from_b58check(&b58check_privkey)?;
    } else {
        secret_key = SecretKey::random(&mut rng);
        fs::write(&network_config.node_key_file, secret_key.into_b58check())?;
    }
    let public_key = PublicKey::from_secret_key(&secret_key);

    // create clonable network parameters
    let network_parameters = NetworkParameters {
        public_key: public_key,
        secret_key: secret_key,
        network_config: network_config
    };
    
    // launch listener process
    let p_listener;
    {
        let network_parameters_clone = network_parameters.clone();
        p_listener = tokio::spawn(async move {
            listener_process(network_parameters_clone).await
        })
    }

    // launch connector
    // TODO
    //  keep n_trusted connection to trusted nodes
    //  keep n_out connections to different IPs known by trusted nodes
    //  keep all incoming connections, with per-IP limit


    // launch
    p_listener.await?
}
