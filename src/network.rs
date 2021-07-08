use crate::config;
use crate::crypto::{B58able, PublicKey, SecretKey};
use failure::Error;
use rand::rngs::OsRng;
use std::fs;
use std::path::Path;
// use tokio::prelude::*;

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

// async fn network_handler(network_config: &config::NetworkConfig, secret_key: &SecretKey, public_key: &PublicKey) -> Result<(), Error> {
//     // declare lists

//     // TODO list of trusted outbound connections
//     // TODO list of outbound connections
//     // TODO list of inbound connections

//     // TODO list of outbound trusted connections (IP AND sig must match)

//     /* Types of messages:
//         * broadcast [in] [max_emits, min_sent] : keep a list of who is aware of what

//     */
//     // launch listener
//     //let mut listener = tokio::net::TcpListener::bind(&network_config.bind).await?;

//     // launch connections to known targets, trusted in priority
//     //connect_to_target(xxxx, trusted=true);  // TODO

//     // loop
//     Ok(())
// }

pub async fn run(network_config: &config::NetworkConfig) -> Result<(), Error> {
    // load node auth key if exists. Otherwise generate a new one and write it to file
    let secret_key;
    if Path::new(&network_config.node_key_file).exists() {
        let b58check_privkey = fs::read_to_string(&network_config.node_key_file)?;
        secret_key = SecretKey::from_b58check(&b58check_privkey)?;
    } else {
        let mut rng = OsRng::default();
        secret_key = SecretKey::random(&mut rng);
        fs::write(&network_config.node_key_file, secret_key.into_b58check())?;
    }
    let _public_key = PublicKey::from_secret_key(&secret_key);

    // launch connection handler
    // network_handler(&network_config, &secret_key, &public_key).await?;

    Ok(())
}
