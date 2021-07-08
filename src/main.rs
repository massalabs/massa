#![feature(ip)]

mod config;
mod crypto;
mod network;
mod protocol;

use log::{error, info};
use std::error::Error;
use tokio::fs::read_to_string;
use tokio::time::{sleep, Duration};

type BoxResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

async fn run(cfg: config::Config) -> BoxResult<()> {
    // launch network controller
    let mut net = network::connection_controller::ConnectionController::new(&cfg.network).await?;
    let mut net_interface = net.get_upstream_interface();

    // loop over messages
    loop {
        tokio::select! {
            evt = net.wait_event() => match evt {
                network::connection_controller::ConnectionControllerEvent::NewConnection((id, socket)) => {
                    info!("new peer: {:#?}", id);
                    sleep(Duration::from_secs(2)).await;
                    net_interface.connection_alive(id).await;
                    sleep(Duration::from_secs(20)).await;
                    net_interface.connection_closed(id, network::connection_controller::ConnectionClosureReason::Normal).await;
                    info!("peer closed: {:#?}", id);
                }
                network::connection_controller::ConnectionControllerEvent::ConnectionBanned(id) => {
                    net_interface.connection_closed(id, network::connection_controller::ConnectionClosureReason::Normal).await;
                    info!("peer closed because of a ban triggered by another connection: {:#?}", id);
                }
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
