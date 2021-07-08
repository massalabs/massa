#![feature(ip)]
#![feature(destructuring_assignment)]

extern crate logging;
mod config;
use api::{start_api_controller, ApiEvent};
use bootstrap::{get_state, start_bootstrap_server};
use communication::{
    network::{start_network_controller, Establisher},
    protocol::start_protocol_controller,
};
use consensus::start_consensus_controller;
use log::{error, info, trace};
use models::SerializationContext;
use storage::start_storage;
use tokio::{
    fs::read_to_string,
    signal::unix::{signal, SignalKind},
};

async fn run(cfg: config::Config) {
    // generate serialization context
    let serialization_context = SerializationContext {
        max_block_operations: cfg.consensus.max_operations_per_block,
        parent_count: cfg.consensus.thread_count,
        max_block_size: cfg.consensus.max_block_size,
        max_peer_list_length: cfg.network.max_advertise_length,
        max_message_size: cfg.network.max_message_size,
        max_bootstrap_blocks: cfg.bootstrap.max_bootstrap_blocks,
        max_bootstrap_cliques: cfg.bootstrap.max_bootstrap_cliques,
        max_bootstrap_deps: cfg.bootstrap.max_bootstrap_deps,
        max_bootstrap_children: cfg.bootstrap.max_bootstrap_children,
        max_ask_blocks_per_message: cfg.network.max_ask_blocks_per_message,
    };

    let (boot_graph, clock_compensation) = get_state(
        cfg.bootstrap.clone(),
        serialization_context.clone(),
        bootstrap::establisher::Establisher::new(),
    )
    .await
    .unwrap();

    // launch network controller
    let (mut network_command_sender, network_event_receiver, network_manager, private_key) =
        start_network_controller(
            cfg.network.clone(),
            serialization_context.clone(),
            Establisher::new(),
            clock_compensation,
        )
        .await
        .expect("could not start network controller");

    let (storage_command_sender, storage_manager) =
        start_storage(cfg.storage.clone(), serialization_context.clone())
            .expect("could not start storage controller");

    // launch protocol controller
    let (protocol_command_sender, protocol_event_receiver, protocol_manager) =
        start_protocol_controller(
            cfg.protocol.clone(),
            serialization_context.clone(),
            network_command_sender.clone(),
            network_event_receiver,
        )
        .await
        .expect("could not start protocol controller");

    // launch consensus controller
    let (consensus_command_sender, mut consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.consensus.clone(),
            serialization_context.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
            Some(storage_command_sender.clone()),
            boot_graph,
            clock_compensation,
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
        clock_compensation,
    )
    .await
    .expect("could not start API controller");

    let bootstrap_manager = start_bootstrap_server(
        consensus_command_sender.clone(),
        cfg.bootstrap,
        serialization_context.clone(),
        bootstrap::Establisher::new(),
        private_key,
    )
    .await
    .unwrap();

    // interrupt signal listener
    let mut stop_signal = signal(SignalKind::interrupt()).unwrap();

    // loop over messages
    loop {
        trace!("waiting on select in loop in massa-node main");
        tokio::select! {
            evt = consensus_event_receiver.wait_event() => {
                trace!("entered consensus_event_receiver.wait_event() branch of the  select in loop in massa-node main");
                match evt {
                    Ok(_) => (),
                    Err(err) => {
                        error!("consensus_event_receiver.wait_event error: {:?}", err);
                        break;
                    }
                }
            },

            evt = api_event_receiver.wait_event() =>{
                trace!("entered api_event_receiver.wait_event() branch of the  select in loop in massa-node main");
                match evt {
                Ok(ApiEvent::AskStop) => {
                    info!("API asked node stop");
                    break;
                },
                Ok(ApiEvent::GetActiveBlock{hash, response_tx}) => {
                    trace!("before sending block to response_tx sender in loop in massa-node main");
                    response_tx.send(
                        consensus_command_sender
                        .get_active_block(hash)
                            .await
                            .expect("could not retrieve block")
                        ).expect("could not send block");

                    trace!("after sending block to response_tx sender in loop in massa-node main");
                },
                Ok(ApiEvent::GetBlockGraphStatus(response_sender_tx)) => {

                    trace!("before sending block graph to response_tx sender in loop in massa-node main");
                    response_sender_tx.send(
                        consensus_command_sender
                        .get_block_graph_status()
                            .await
                            .expect("could not retrive graph status")
                        ).expect("could not send graph status");
                        trace!("after sending block graph to response_tx sender in loop in massa-node main");
                },
                Ok(ApiEvent::GetPeers(response_sender_tx)) => {
                    trace!("before sending peers to response_tx sender in loop in massa-node main");
                    response_sender_tx.send(
                        network_command_sender
                            .get_peers()
                            .await
                            .expect("could not retrive peers")
                        ).expect("could not send peers");

                    trace!("before sending peers to response_tx sender in loop in massa-node main");
                    },
                Ok(ApiEvent::GetSelectionDraw { start, end, response_tx}) => {
                    trace!("before sending selection draws to response_tx sender in loop in massa-node main");
                    response_tx.send(
                        consensus_command_sender
                            .get_selection_draws(start, end )
                            .await
                        ).expect("could not send selection draws");
                        trace!("after sending selection draws to response_tx sender in loop in massa-node main");
                    },

                Err(err) => {
                    error!("api communication error: {:?}", err);
                    break;
                }
            }},

            _ = stop_signal.recv() => {
                trace!("entered stop_signal.recv() branch of the  select in loop in massa-node main");
                info!("interrupt signal received");
                break;
            }
        }
    }

    // stop bootstrap
    if let Some(bootstrap_manager) = bootstrap_manager {
        bootstrap_manager
            .stop()
            .await
            .expect("bootstrap server shutdown failed")
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

    //stop storage controller
    storage_manager
        .stop()
        .await
        .expect("storage shutdown failed");

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
        .module("bootstrap")
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
