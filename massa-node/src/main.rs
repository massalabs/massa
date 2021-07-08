#![feature(ip)]
#![feature(destructuring_assignment)]

extern crate logging;
pub mod config;

use api::ApiEvent;
use communication::network::default_establisher::DefaultEstablisher;
use communication::network::default_network_controller::DefaultNetworkController;
use communication::protocol::default_protocol_controller::DefaultProtocolController;
use consensus::consensus_controller::ConsensusController;
use consensus::default_consensus_controller::DefaultConsensusController;
use log::{error, info, warn};
use tokio::{
    fs::read_to_string,
    signal::unix::{signal, SignalKind},
};

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
    let mut api_handle = api::spawn_server(
        cnss_interface,
        cfg.api.clone(),
        cfg.consensus.clone(),
        cfg.network.clone(),
    )
    .await
    .expect("could not start API");

    let mut stop_signal = signal(SignalKind::interrupt()).unwrap();
    // loop over messages
    loop {
        tokio::select! {
            evt = cnss.wait_event() => match evt {
                _ => {}
            },

            evt = api_handle.wait_event() => match evt {
                Ok(ApiEvent::AskStop) => {
                    info!("shutting down node");
                    break;
                },
                Err(err) => {
                    error!("api communication error: {:?}", err);
                    break;
                }
            },

            _ = stop_signal.recv() => {
                info!("shutting down node");
                break;
            }
        }
    }

    if let Err(e) = api_handle.stop().await {
        warn!("graceful api shutdown failed: {:?}", e);
    }
    if let Err(e) = cnss.stop().await {
        warn!("graceful shutdown failed: {:?}", e);
    }
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
        .module("api")
        .verbosity(cfg.logging.level)
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()
        .unwrap();

    run(cfg).await
}
