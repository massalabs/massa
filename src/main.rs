#![feature(ip)]

#[macro_use]
pub mod logging;
pub mod config;
pub mod consensus;
pub mod crypto;
pub mod network;
pub mod protocol;
pub mod structures;
use crate::consensus::default_consensus_controller::DefaultConsensusController;
use crate::logging::{debug, error};
use crate::network::default_establisher::DefaultEstablisher;
use crate::network::default_network_controller::DefaultNetworkController;
use crate::protocol::default_protocol_controller::DefaultProtocolController;
use consensus::consensus_controller::ConsensusController;
use std::error::Error;
use tokio::fs::read_to_string;

type BoxResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

async fn run(cfg: config::Config) -> BoxResult<()> {
    let establisher = DefaultEstablisher::new();
    let network = DefaultNetworkController::new(&cfg.network, establisher).await?;

    // launch consensus controller
    let ptcl = DefaultProtocolController::new(cfg.protocol, network).await?;
    let mut cnss = DefaultConsensusController::new(&cfg.consensus, ptcl).await?;

    let mut wait_init = tokio::time::sleep(tokio::time::Duration::from_millis(10000));

    // loop over messages
    loop {
        tokio::select! {
            evt = cnss.wait_event() => match evt {
                _ => {}
            },
            _ = &mut wait_init, if !wait_init.is_elapsed() => {
                debug!("requiring random block generation");
                cnss.generate_random_block().await;
            }
        }
    }

    //Ok(())
    /* TODO uncomment when it becomes reachable again
    if let Err(e) = cnss.stop().await {
        warn!("graceful shutdown failed: {}", e);
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
