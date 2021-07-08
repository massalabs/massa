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
use logging::{massa_trace, warn};
use models::{init_serialization_context, SerializationContext};
use pool::start_pool_controller;
use storage::start_storage;
use tokio::{
    fs::read_to_string,
    signal::unix::{signal, SignalKind},
};

async fn run(cfg: config::Config) {
    // Init the global serialization context
    init_serialization_context(SerializationContext {
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
        max_operations_per_message: cfg.network.max_operations_per_message,
        max_bootstrap_message_size: cfg.bootstrap.max_bootstrap_message_size,
        max_bootstrap_pos_cycles: cfg.bootstrap.max_bootstrap_pos_cycles,
        max_bootstrap_pos_entries: cfg.bootstrap.max_bootstrap_pos_entries,
    });

    let (boot_pos, boot_graph, clock_compensation, initial_peers) = get_state(
        cfg.bootstrap.clone(),
        bootstrap::establisher::Establisher::new(),
    )
    .await
    .unwrap();

    // launch network controller
    let (network_command_sender, network_event_receiver, network_manager, private_key) =
        start_network_controller(
            cfg.network.clone(),
            Establisher::new(),
            clock_compensation,
            initial_peers,
        )
        .await
        .expect("could not start network controller");

    // start storage
    let (storage_command_sender, storage_manager) =
        start_storage(cfg.storage.clone()).expect("could not start storage controller");

    // launch protocol controller
    let (
        protocol_command_sender,
        protocol_event_receiver,
        protocol_pool_event_receiver,
        protocol_manager,
    ) = start_protocol_controller(
        cfg.protocol.clone(),
        cfg.consensus.operation_validity_periods.clone(),
        network_command_sender.clone(),
        network_event_receiver,
    )
    .await
    .expect("could not start protocol controller");

    // launch pool controller
    let (pool_command_sender, pool_manager) = start_pool_controller(
        cfg.pool.clone(),
        cfg.consensus.thread_count,
        cfg.consensus.operation_validity_periods,
        protocol_command_sender.clone(),
        protocol_pool_event_receiver,
    )
    .await
    .expect("could not start pool controller");

    // launch consensus controller
    let (consensus_command_sender, mut consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.consensus.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
            pool_command_sender.clone(),
            Some(storage_command_sender.clone()),
            boot_pos,
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
        cfg.pool.clone(),
        Some(storage_command_sender),
        clock_compensation,
    )
    .await
    .expect("could not start API controller");

    let bootstrap_manager = start_bootstrap_server(
        consensus_command_sender.clone(),
        network_command_sender.clone(),
        cfg.bootstrap,
        bootstrap::Establisher::new(),
        private_key,
        clock_compensation,
    )
    .await
    .unwrap();

    // interrupt signal listener
    let mut stop_signal = signal(SignalKind::interrupt()).unwrap();

    // loop over messages
    loop {
        massa_trace!("massa-node.main.run.select", {});
        let mut api_pool_command_sender = pool_command_sender.clone();
        tokio::select! {
            evt = consensus_event_receiver.wait_event() => {
                massa_trace!("massa-node.main.run.select.consensus_event", {});
                match evt {
                    Ok(_) => (),
                    Err(err) => {
                        error!("consensus_event_receiver.wait_event error: {:?}", err);
                        break;
                    }
                }
            },

            evt = api_event_receiver.wait_event() =>{
                massa_trace!("massa-node.main.run.select.api_event", {});
                match evt {
                Ok(ApiEvent::AddOperations(operations)) => {
                    massa_trace!("massa-node.main.run.select.api_event.AddOperations", {"operations": operations});
                    if api_pool_command_sender.add_operations(operations)
                            .await
                        .is_err() {
                            warn!("could not send AddOperations to pool in api_event_receiver.wait_event");
                        }

                },
                Ok(ApiEvent::AskStop) => {
                    info!("API asked node stop");
                    break;
                },
                Ok(ApiEvent::GetActiveBlock{block_id, response_tx}) => {
                    massa_trace!("massa-node.main.run.select.api_event.get_active_block", {"block_id": block_id});
                    if response_tx.send(
                        consensus_command_sender
                        .get_active_block(block_id)
                            .await
                            .expect("get_active_block failed in api_event_receiver.wait_event")
                        ).is_err() {
                            warn!("could not send get_active_block response in api_event_receiver.wait_event");
                        }
                },
                Ok(ApiEvent::GetBlockGraphStatus(response_sender_tx)) => {
                    massa_trace!("massa-node.main.run.select.api_event.get_block_graph_status", {});
                    if response_sender_tx.send(
                        consensus_command_sender
                        .get_block_graph_status()
                            .await
                            .expect("get_block_graph_status failed in api_event_receiver.wait_event")
                        ).is_err() {
                            warn!("could not send get_block_graph_status response in api_event_receiver.wait_event");
                        }
                        trace!("after sending block graph to response_tx sender in loop in massa-node main");
                },
                Ok(ApiEvent::GetPeers(response_sender_tx)) => {
                    massa_trace!("massa-node.main.run.select.api_event.get_peers", {});
                    if response_sender_tx.send(
                        network_command_sender
                            .get_peers()
                            .await
                            .expect("get_peers failed in api_event_receiver.wait_event")
                        ).is_err() {
                            warn!("could not send get_peers response in api_event_receiver.wait_event");
                        }
                    },
                Ok(ApiEvent::GetSelectionDraw {start, end, response_tx}) => {
                    massa_trace!("massa-node.main.run.select.api_event.get_selection_draws", {});
                    if response_tx.send(
                        consensus_command_sender
                            .get_selection_draws(start, end)
                            .await
                        ).is_err() {
                            warn!("could not send get_selection_draws response in api_event_receiver.wait_event");
                        }
                    },
                Ok(ApiEvent::GetRollState {addresses, response_tx}) => {
                    massa_trace!("massa-node.main.run.select.api_event.get_selection_draws", {});
                    if response_tx.send(
                        consensus_command_sender
                            .get_roll_state(addresses)
                            .await
                            .expect("could not get roll state")
                        ).is_err() {
                            warn!("could not send get_selection_draws response in api_event_receiver.wait_event");
                        }
                    },
                Ok(ApiEvent::GetLedgerData {addresses, response_tx}) => {
                    massa_trace!("massa-node.main.run.select.api_event.get_ledger_data", {});
                    if response_tx.send(
                        consensus_command_sender
                            .get_ledger_data(addresses )
                            .await
                        ).is_err() {
                            warn!("could not send get_sledger_data response in api_event_receiver.wait_event");
                        }
                    },
                Ok(ApiEvent::GetRecentOperations {address, response_tx}) => {
                    massa_trace!("massa-node.main.run.select.api_event.get_operations_involving_address", {});
                    if response_tx.send(
                        consensus_command_sender
                            .get_operations_involving_address(address )
                            .await
                            .expect("could not get recent operations")
                        ).is_err() {
                            warn!("could not send get_operations_involving_address response in api_event_receiver.wait_event");
                        }
                    },
                Ok(ApiEvent::GetOperations {operation_ids, response_tx}) => {
                    massa_trace!("massa-node.main.run.select.api_event.get_operations", {"operation_ids": operation_ids});
                    if response_tx.send(
                        consensus_command_sender
                            .get_operations(operation_ids)
                            .await
                            .expect("could not get operations".into())
                        ).is_err() {
                            warn!("could not send get_operations response in api_event_receiver.wait_event");
                        }
                    },
                Err(err) => {
                    error!("api communication error: {:?}", err);
                    break;
                }
            }},

            _ = stop_signal.recv() => {
                massa_trace!("massa-node.main.run.select.stop", {});
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

    // stop pool controller
    let protocol_pool_event_receiver = pool_manager.stop().await.expect("pool shutdown failed");

    // stop protocol controller
    let network_event_receiver = protocol_manager
        .stop(protocol_event_receiver, protocol_pool_event_receiver)
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
        .module("pool")
        .verbosity(cfg.logging.level)
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()
        .unwrap();

    run(cfg).await
}
