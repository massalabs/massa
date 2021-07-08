use failure::Error;
use crate::config;
use crate::crypto::{B58able, SecretKey, PublicKey};
use tokio::prelude::*;
use std::path::Path;
use std::fs;
use rand::rngs::OsRng;


async fn listen(bind: &String) -> Result<(), Error> {
    let mut listener = tokio::net::TcpListener::bind(bind).await?;
    loop {
        let (mut socket, _) = listener.accept().await?;
        tokio::spawn(async move {
            let mut buf = [0; 1024];
            loop {
                // TODO here perform Receive handshake
                // ---
                
                let n = match socket.read(&mut buf).await {
                    // socket closed
                    Ok(0) => return,
                    Ok(n) => n,
                    Err(e) => {
                        println!("failed to read from socket; err = {:?}", e);
                        return;
                    }
                };

                // Write the data back
                if let Err(e) = socket.write_all(&buf[0..n]).await {
                    println!("failed to write to socket; err = {:?}", e);
                    return;
                }
            }
        });
        
    }
}

async fn handle_connections(network_config: &config::NetworkConfig) -> Result<(), Error> {
    listen(&network_config.bind).await?;
    Ok(())
}

pub async fn initialize(network_config: &config::NetworkConfig) -> Result<(), Error> {
    // init rng
    let mut rng = OsRng::default();

    // load node key if exists. Otherwise generate a new one and write it to file
    let secret_key;
    if Path::new(&network_config.node_key_file).exists() {
        let b58check_privkey = fs::read_to_string(&network_config.node_key_file)?;
        secret_key = SecretKey::from_b58check(&b58check_privkey)?;
    } else {
        secret_key = SecretKey::random(&mut rng);
        fs::write(&network_config.node_key_file, secret_key.into_b58check())?;
    }
    let public_key = PublicKey::from_secret_key(&secret_key);
    

    // launch connection handler
    handle_connections(network_config).await?;

    Ok(())
}




