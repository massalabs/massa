#![feature(ip)]

mod config;
mod crypto;
mod network;
mod protocol;

use crate::protocol::controller::{ProtocolController, ProtocolEvent};
use log::error;
use std::error::Error;
use tokio::fs::read_to_string;

type BoxResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

async fn run(cfg: config::Config) -> BoxResult<()> {
    // launch network controller
    let mut protocol = ProtocolController::new(&cfg.protocol).await?;

    // loop over messages
    loop {
        tokio::select! {
            evt = protocol.wait_event() => match evt {
                ProtocolEvent::ReceivedTransaction(data) => log::info!("reveice transcation with date:{}", data),
                ProtocolEvent::ReceivedBlock(data) => log::info!("reveice a block with date:{}", data),
             }
        }
    }

    /* TODO uncomment when it becomes reachable again
    if let Err(e) = protocol.stop().await {
        warn!("graceful protocol shutdown failed: {}", e);
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
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()
        .unwrap();

    match run(cfg).await {
        Ok(_) => {}
        Err(e) => {
            error!("error in program root: {}", e);
        }
    }
}
