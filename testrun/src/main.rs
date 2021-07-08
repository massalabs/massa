#![feature(ip)]
#![feature(destructuring_assignment)]

extern crate logging;
pub mod config;

use communication::network::default_establisher::DefaultEstablisher;
use communication::network::default_network_controller::DefaultNetworkController;
use communication::protocol::default_protocol_controller::DefaultProtocolController;
use consensus::consensus_controller::ConsensusController;
use consensus::default_consensus_controller::DefaultConsensusController;
use tokio::fs::read_to_string;

async fn run(cfg: config::Config) -> () {
    let establisher = DefaultEstablisher::new();
    let network = DefaultNetworkController::new(&cfg.network, establisher)
        .await
        .expect("Could not create network controller");

    // launch consensus controller
    let ptcl = DefaultProtocolController::new(cfg.protocol.clone(), network).await;
    let mut cnss = DefaultConsensusController::new(&cfg.consensus, ptcl)
        .await
        .expect("Could not create consensus controller");

    // spawn API
    let cnss_interface = cnss.get_interface();
    let api_handle = tokio::spawn(async move {
        api::serve(cnss_interface, cfg.consensus.clone(), cfg.network.clone()).await;
    });

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
        .module("communication")
        .module("consensus")
        .module("crypto")
        .module("logging")
        .module("models")
        .module("time")
        .verbosity(cfg.logging.level)
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()
        .unwrap();

    run(cfg).await
}
