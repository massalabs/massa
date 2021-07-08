#![feature(ip)]
#![feature(destructuring_assignment)]

extern crate logging;
mod config;
use storage::storage_controller::start_storage_controller;

use api::{start_api_controller, ApiEvent};
use communication::{
    network::{start_network_controller, Establisher},
    protocol::start_protocol_controller,
};
use consensus::start_consensus_controller;
use log::{error, info};
use tokio::{
    fs::read_to_string,
    signal::unix::{signal, SignalKind},
};

async fn run(cfg: config::Config) {
    // launch network controller
    let (mut network_command_sender, network_event_receiver, network_manager) =
        start_network_controller(cfg.network.clone(), Establisher::new())
            .await
            .expect("could not start network controller");

    let (storage_command_sender, storage_manager) =
        start_storage_controller(cfg.storage).expect("could not start storage controller");

    // launch protocol controller
    let (protocol_command_sender, protocol_event_receiver, protocol_manager) =
        start_protocol_controller(
            cfg.protocol.clone(),
            network_command_sender.clone(),
            network_event_receiver,
            Some(storage_command_sender.clone()),
        )
        .await
        .expect("could not start protocol controller");

    // launch consensus controller
    let (consensus_command_sender, mut consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.consensus.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
            Some(storage_command_sender.clone()),
        )
        .await
        .expect("could not start consensus controller");

    // launch API controller
    let (mut api_event_receiver, api_manager) = start_api_controller(
        cfg.api.clone(),
        cfg.consensus.clone(),
        cfg.protocol.clone(),
        cfg.network.clone(),
        Some(storage_command_sender),
    )
    .await
    .expect("could not start API controller");

    // interrupt signal listener
    let mut stop_signal = signal(SignalKind::interrupt()).unwrap();

    // loop over messages
    loop {
        tokio::select! {
            evt = consensus_event_receiver.wait_event() => match evt {
                _ => {}
            },

            evt = api_event_receiver.wait_event() => match evt {
                Ok(ApiEvent::AskStop) => {
                    info!("API asked node stop");
                    break;
                },
                Ok(ApiEvent::GetActiveBlock(hash, response_sender_tx)) => {
                    response_sender_tx.send(
                        consensus_command_sender
                        .get_active_block(hash)
                            .await
                            .expect("could not retrieve block")
                        ).expect("could not send block");
                },
                Ok(ApiEvent::GetBlockGraphStatus(response_sender_tx)) => {
                    response_sender_tx.send(
                        consensus_command_sender
                        .get_block_graph_status()
                            .await
                            .expect("could not retrive graph status")
                        ).expect("could not send graph status");
                },
                Ok(ApiEvent::GetPeers(response_sender_tx)) => {
                    response_sender_tx.send(
                        network_command_sender
                            .get_peers()
                            .await
                            .expect("could not retrive peers")
                        ).expect("could not send peers");
                    },
                Ok(ApiEvent::GetSelectionDraw(start, end, response_sender_tx)) => {
                    response_sender_tx.send(
                        consensus_command_sender
                            .get_selection_draws(start, end )
                            .await
                        ).expect("could not send selection draws");
                    },

                Err(err) => {
                    error!("api communication error: {:?}", err);
                    break;
                }
            },

            _ = stop_signal.recv() => {
                info!("interrupt signal received");
                break;
            }
        }
    }

    // stop API controller
    let _remaining_api_events = api_manager
        .stop(api_event_receiver)
        .await
        .expect("API shutdown failed");

    // stop consensus controller
    let protocol_event_receiver = consensus_manager
        .stop(consensus_event_receiver)
        .await
        .expect("consensus shutdown failed");

    // stop protocol controller
    let network_event_receiver = protocol_manager
        .stop(protocol_event_receiver)
        .await
        .expect("protocol shutdown failed");

    // stop network controller
    network_manager
        .stop(network_event_receiver)
        .await
        .expect("network shutdown failed");
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
