#![feature(ip)]



mod config;
mod network;
mod protocol;
mod crypto;


use log::{error, info};
use std::error::Error;
use tokio::fs::read_to_string;
use tokio::time::{sleep, Duration};

type BoxResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

async fn run(cfg: config::Config) -> BoxResult<()> {
    // launch network controller
    let mut net = network::controller::NetworkController::new(cfg.network).await?;
    let mut net_interface = net.get_upstream_interface();

    // loop over messages
    loop {
        tokio::select! {
            evt = net.wait_event() => match evt {
                Ok(msg) => match msg {
                    network::controller::NetworkControllerEvent::CandidateConnection {ip, socket} => {
                        info!("new peer: {}", ip);
                        sleep(Duration::from_secs(2)).await;
                        net_interface.peer_alive(ip).await;
                        sleep(Duration::from_secs(20)).await;
                        net_interface.peer_closed(ip, network::controller::PeerClosureReason::Normal).await;
                        info!("peer closed: {}", ip);
                    }
                },
                Err(e) => return Err(e)
            }
        }
    }

    /* TODO uncomment when it becomes reachable again
    if let Err(e) = net.stop().await {
        warn!("graceful network shutdown failed: {}", e);
    }
    Ok(())
    */
}

#[tokio::main]
async fn main() {
    // load config
    let config_path = "config/config.toml";
    let cfg = config::Config::from_toml(&read_to_string(config_path).await.unwrap()).unwrap();

    // setup logging
    stderrlog::new()
        .module(module_path!())
        .verbosity(cfg.logging.level)
        .init()
        .unwrap();

    match run(cfg).await {
        Ok(_) => {}
        Err(e) => {
            error!("error in program root: {}", e);
        }
    }
}
