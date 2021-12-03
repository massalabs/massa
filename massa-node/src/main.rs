// Copyright (c) 2021 MASSA LABS <info@massa.net>

#![feature(ip)]
#![feature(destructuring_assignment)]
#![doc = include_str!("../../README.md")]

extern crate logging;

use crate::settings::SETTINGS;
use api::{Private, Public, RpcServer, StopHandle, API};
use bootstrap::{get_state, start_bootstrap_server, BootstrapManager};
use consensus::{
    start_consensus_controller, ConsensusCommandSender, ConsensusEvent, ConsensusEventReceiver,
    ConsensusManager,
};
use logging::massa_trace;
use models::{init_serialization_context, SerializationContext};
use network::{start_network_controller, Establisher, NetworkCommandSender, NetworkManager};
use pool::{start_pool_controller, PoolCommandSender, PoolManager};
use protocol_exports::ProtocolManager;
use protocol_worker::start_protocol_controller;
use std::process;
use time::UTime;
use tokio::signal;
use tokio::sync::mpsc;
use tracing::{error, info, warn, Level};

mod settings;

async fn launch() -> (
    PoolCommandSender,
    ConsensusEventReceiver,
    ConsensusCommandSender,
    NetworkCommandSender,
    Option<BootstrapManager>,
    ConsensusManager,
    PoolManager,
    ProtocolManager,
    NetworkManager,
    mpsc::Receiver<()>,
    StopHandle,
    StopHandle,
) {
    info!("Node version : {}", SETTINGS.version);
    if let Some(end) = SETTINGS.consensus.end_timestamp {
        if UTime::now(0).expect("could not get now time") > end {
            panic!("This episode has come to an end, please get the latest testnet node version to continue");
        }
    }

    // Init the global serialization context
    init_serialization_context(SerializationContext {
        max_block_operations: SETTINGS.consensus.max_operations_per_block,
        parent_count: SETTINGS.consensus.thread_count,
        max_block_size: SETTINGS.consensus.max_block_size,
        max_peer_list_length: SETTINGS.network.max_advertise_length,
        max_message_size: SETTINGS.network.max_message_size,
        max_bootstrap_blocks: SETTINGS.bootstrap.max_bootstrap_blocks,
        max_bootstrap_cliques: SETTINGS.bootstrap.max_bootstrap_cliques,
        max_bootstrap_deps: SETTINGS.bootstrap.max_bootstrap_deps,
        max_bootstrap_children: SETTINGS.bootstrap.max_bootstrap_children,
        max_ask_blocks_per_message: SETTINGS.network.max_ask_blocks_per_message,
        max_operations_per_message: SETTINGS.network.max_operations_per_message,
        max_endorsements_per_message: SETTINGS.network.max_endorsements_per_message,
        max_bootstrap_message_size: SETTINGS.bootstrap.max_bootstrap_message_size,
        max_bootstrap_pos_cycles: SETTINGS.bootstrap.max_bootstrap_pos_cycles,
        max_bootstrap_pos_entries: SETTINGS.bootstrap.max_bootstrap_pos_entries,
        max_block_endorsements: SETTINGS.consensus.endorsement_count,
    });

    // interrupt signal listener
    let stop_signal = signal::ctrl_c();
    tokio::pin!(stop_signal);
    let (boot_pos, boot_graph, clock_compensation, initial_peers) = tokio::select! {
        _ = &mut stop_signal => {
            info!("interrupt signal received in bootstrap loop");
            process::exit(0);
        },
        res = get_state(
            &SETTINGS.bootstrap,
            bootstrap::establisher::Establisher::new(),
            SETTINGS.version,
            SETTINGS.consensus.genesis_timestamp,
            SETTINGS.consensus.end_timestamp,
        ) => match res {
            Ok(vals) => vals,
            Err(err) => panic!("critical error detected in the bootstrap process: {}", err)
        }
    };

    // launch network controller
    let (network_command_sender, network_event_receiver, network_manager, private_key, node_id) =
        start_network_controller(
            SETTINGS.network.clone(), // TODO: get rid of this clone() ... see #1277
            Establisher::new(),
            clock_compensation,
            initial_peers,
            SETTINGS.version,
        )
        .await
        .expect("could not start network controller");

    // launch protocol controller
    let (
        protocol_command_sender,
        protocol_event_receiver,
        protocol_pool_event_receiver,
        protocol_manager,
    ) = start_protocol_controller(
        &SETTINGS.protocol,
        SETTINGS.consensus.operation_validity_periods,
        network_command_sender.clone(),
        network_event_receiver,
    )
    .await
    .expect("could not start protocol controller");

    // launch pool controller
    let (pool_command_sender, pool_manager) = start_pool_controller(
        &SETTINGS.pool,
        SETTINGS.consensus.thread_count,
        SETTINGS.consensus.operation_validity_periods,
        protocol_command_sender.clone(),
        protocol_pool_event_receiver,
    )
    .await
    .expect("could not start pool controller");

    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            SETTINGS.consensus.clone(), // TODO: get rid of this clone() ... see #1277
            protocol_command_sender.clone(),
            protocol_event_receiver,
            pool_command_sender.clone(),
            boot_pos,
            boot_graph,
            clock_compensation,
        )
        .await
        .expect("could not start consensus controller");

    // launch bootstrap server
    let bootstrap_manager = start_bootstrap_server(
        consensus_command_sender.clone(),
        network_command_sender.clone(),
        &SETTINGS.bootstrap,
        bootstrap::Establisher::new(),
        private_key,
        clock_compensation,
        SETTINGS.version,
    )
    .await
    .unwrap();

    // spawn private API
    let (api_private, api_private_stop_rx) = API::<Private>::new(
        consensus_command_sender.clone(),
        network_command_sender.clone(),
        &SETTINGS.api,
        &SETTINGS.consensus,
    );
    let api_private_handle = api_private.serve(&SETTINGS.api.bind_private);

    // spawn public API
    let api_public = API::<Public>::new(
        consensus_command_sender.clone(),
        &SETTINGS.api,
        &SETTINGS.consensus,
        pool_command_sender.clone(),
        &SETTINGS.network,
        SETTINGS.version,
        network_command_sender.clone(),
        clock_compensation,
        node_id,
    );
    let api_public_handle = api_public.serve(&SETTINGS.api.bind_public);

    (
        pool_command_sender,
        consensus_event_receiver,
        consensus_command_sender,
        network_command_sender,
        bootstrap_manager,
        consensus_manager,
        pool_manager,
        protocol_manager,
        network_manager,
        api_private_stop_rx,
        api_private_handle,
        api_public_handle,
    )
}

async fn stop(
    bootstrap_manager: Option<BootstrapManager>,
    consensus_manager: ConsensusManager,
    consensus_event_receiver: ConsensusEventReceiver,
    pool_manager: PoolManager,
    protocol_manager: ProtocolManager,
    network_manager: NetworkManager,
    api_private_handle: StopHandle,
    api_public_handle: StopHandle,
) {
    // stop bootstrap
    if let Some(bootstrap_manager) = bootstrap_manager {
        bootstrap_manager
            .stop()
            .await
            .expect("bootstrap server shutdown failed")
    }

    // stop public API
    api_public_handle.stop();

    // stop private API
    api_private_handle.stop();

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

    // stop network controller
    network_manager
        .stop(network_event_receiver)
        .await
        .expect("network shutdown failed");
}

#[tokio::main]
async fn main() {
    // setup logging
    tracing_subscriber::fmt()
        .with_max_level(match SETTINGS.logging.level {
            4 => Level::TRACE,
            3 => Level::DEBUG,
            2 => Level::INFO,
            1 => Level::WARN,
            _ => Level::ERROR,
        })
        .init();

    // run
    loop {
        let (
            _pool_command_sender,
            mut consensus_event_receiver,
            _consensus_command_sender,
            _network_command_sender,
            bootstrap_manager,
            consensus_manager,
            pool_manager,
            protocol_manager,
            network_manager,
            mut api_private_stop_rx,
            api_private_handle,
            api_public_handle,
        ) = launch().await;

        // interrupt signal listener
        let stop_signal = signal::ctrl_c();
        tokio::pin!(stop_signal);
        // loop over messages
        let restart = loop {
            massa_trace!("massa-node.main.run.select", {});
            tokio::select! {
                evt = consensus_event_receiver.wait_event() => {
                    massa_trace!("massa-node.main.run.select.consensus_event", {});
                    match evt {
                        Ok(ConsensusEvent::NeedSync) => {
                            warn!("in response to a desynchronization, the node is going to bootstrap again");
                            break true;
                        },
                        Err(err) => {
                            error!("consensus_event_receiver.wait_event error: {}", err);
                            break false;
                        }
                    }
                },

                _ = &mut stop_signal => {
                    massa_trace!("massa-node.main.run.select.stop", {});
                    info!("interrupt signal received");
                    break false;
                }

                _ = api_private_stop_rx.recv() => {
                    info!("stop command received from private API");
                    break false;
                }
            }
        };
        stop(
            bootstrap_manager,
            consensus_manager,
            consensus_event_receiver,
            pool_manager,
            protocol_manager,
            network_manager,
            api_private_handle,
            api_public_handle,
        )
        .await;

        if !restart {
            break;
        }
    }
}
