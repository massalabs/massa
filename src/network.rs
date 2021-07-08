use failure::Error;
use crate::config;
use tokio::prelude::*;

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

pub async fn initialize(network_config: &config::NetworkConfig) -> Result<(), Error> {
    listen(&network_config.bind).await?;
    Ok(())
}




