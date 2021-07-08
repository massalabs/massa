#![feature(ip)]
#![feature(destructuring_assignment)]

#[macro_use]
extern crate logging;
pub mod config;

use communication::network::default_establisher::DefaultEstablisher;
use communication::network::default_network_controller::DefaultNetworkController;
use communication::protocol::default_protocol_controller::DefaultProtocolController;
use consensus::consensus_controller::ConsensusController;
use consensus::default_consensus_controller::DefaultConsensusController;
use std::error::Error;
use tokio::fs::read_to_string;

type BoxResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

async fn run(cfg: config::Config) -> BoxResult<()> {
    let establisher = DefaultEstablisher::new();
    let network = DefaultNetworkController::new(&cfg.network, establisher).await?;

    // launch consensus controller
    let ptcl = DefaultProtocolController::new(cfg.protocol, network).await?;
    let mut cnss = DefaultConsensusController::new(&cfg.consensus, ptcl).await?;

    // loop over messages
    loop {
        tokio::select! {
            evt = cnss.wait_event() => match evt {
                _ => {}
            },
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
