// Copyright (c) 2021 MASSA LABS <info@massa.net>

#![feature(ip)]
#![doc = include_str!("../../README.md")]

extern crate massa_logging;
use crate::settings::SETTINGS;
use massa_api::{Private, Public, RpcServer, StopHandle, API};
use massa_bootstrap::{get_state, start_bootstrap_server, BootstrapManager};
use massa_consensus::{
    start_consensus_controller, ConsensusChannels, ConsensusCommandSender, ConsensusEvent,
    ConsensusEventReceiver, ConsensusManager,
};

use massa_execution::{ExecutionConfigs, ExecutionManager};

use massa_logging::massa_trace;
use massa_models::{init_serialization_context, SerializationContext};
use massa_network::{start_network_controller, Establisher, NetworkCommandSender, NetworkManager};
use massa_pool::{start_pool_controller, PoolCommandSender, PoolManager};
use massa_protocol_exports::ProtocolManager;
use massa_protocol_worker::start_protocol_controller;
use massa_time::MassaTime;
use std::process;
use tokio::signal;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use tracing_subscriber::{
    filter::{filter_fn, LevelFilter},
    prelude::*,
};

mod settings;

async fn launch() -> (
    PoolCommandSender,
    ConsensusEventReceiver,
    ConsensusCommandSender,
    NetworkCommandSender,
    Option<BootstrapManager>,
    ConsensusManager,
    ExecutionManager,
    PoolManager,
    ProtocolManager,
    NetworkManager,
    mpsc::Receiver<()>,
    StopHandle,
    StopHandle,
) {
    info!("Node version : {}", *crate::settings::VERSION);
    if let Some(end) = *massa_consensus::settings::END_TIMESTAMP {
        if MassaTime::now().expect("could not get now time") > end {
            panic!("This episode has come to an end, please get the latest testnet node version to continue");
        }
    }

    // Init the global serialization context
    init_serialization_context(SerializationContext {
        max_block_operations: massa_consensus::settings::MAX_OPERATIONS_PER_BLOCK,
        parent_count: massa_consensus::settings::THREAD_COUNT,
        max_block_size: massa_consensus::settings::MAX_BLOCK_SIZE,
        max_block_endorsements: massa_consensus::settings::ENDORSEMENT_COUNT,
        max_peer_list_length: massa_network::settings::MAX_ADVERTISE_LENGTH,
        max_message_size: massa_network::settings::MAX_MESSAGE_SIZE,
        max_bootstrap_blocks: massa_bootstrap::settings::MAX_BOOTSTRAP_BLOCKS,
        max_bootstrap_cliques: massa_bootstrap::settings::MAX_BOOTSTRAP_CLIQUES,
        max_bootstrap_deps: massa_bootstrap::settings::MAX_BOOTSTRAP_DEPS,
        max_bootstrap_children: massa_bootstrap::settings::MAX_BOOTSTRAP_CHILDREN,
        max_ask_blocks_per_message: massa_network::settings::MAX_ASK_BLOCKS_PER_MESSAGE,
        max_operations_per_message: massa_network::settings::MAX_OPERATIONS_PER_MESSAGE,
        max_endorsements_per_message: massa_network::settings::MAX_ENDORSEMENTS_PER_MESSAGE,
        max_bootstrap_message_size: massa_bootstrap::settings::MAX_BOOTSTRAP_MESSAGE_SIZE,
        max_bootstrap_pos_cycles: massa_bootstrap::settings::MAX_BOOTSTRAP_POS_CYCLES,
        max_bootstrap_pos_entries: massa_bootstrap::settings::MAX_BOOTSTRAP_POS_ENTRIES,
    });

    // interrupt signal listener
    let stop_signal = signal::ctrl_c();
    tokio::pin!(stop_signal);
    let bootstrap_state = tokio::select! {
        _ = &mut stop_signal => {
            info!("interrupt signal received in bootstrap loop");
            process::exit(0);
        },
        res = get_state(
            &SETTINGS.bootstrap,
            massa_bootstrap::establisher::Establisher::new(),
            *settings::VERSION,
            *massa_consensus::settings::GENESIS_TIMESTAMP,
            *massa_consensus::settings::END_TIMESTAMP,
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
            bootstrap_state.compensation_millis,
            bootstrap_state.peers,
            *crate::settings::VERSION,
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
        massa_consensus::settings::OPERATION_VALIDITY_PERIODS,
        massa_consensus::settings::MAX_GAS_PER_BLOCK,
        network_command_sender.clone(),
        network_event_receiver,
    )
    .await
    .expect("could not start protocol controller");

    // launch pool controller
    let (pool_command_sender, pool_manager) = start_pool_controller(
        &SETTINGS.pool,
        massa_consensus::settings::THREAD_COUNT,
        massa_consensus::settings::OPERATION_VALIDITY_PERIODS,
        protocol_command_sender.clone(),
        protocol_pool_event_receiver,
    )
    .await
    .expect("could not start pool controller");

    let execution_config = ExecutionConfigs {
        settings: SETTINGS.execution.clone(),
        thread_count: massa_consensus::settings::THREAD_COUNT,
        genesis_timestamp: *massa_consensus::settings::GENESIS_TIMESTAMP,
        t0: *massa_consensus::settings::T0,
        clock_compensation: bootstrap_state.compensation_millis,
    };

    // launch execution controller
    let (execution_command_sender, execution_event_receiver, execution_manager) =
        massa_execution::start_controller(execution_config, bootstrap_state.execution)
            .await
            .expect("could not start execution controller");

    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            SETTINGS.consensus.config(),
            ConsensusChannels {
                execution_command_sender: execution_command_sender.clone(),
                execution_event_receiver,
                protocol_command_sender: protocol_command_sender.clone(),
                protocol_event_receiver,
                pool_command_sender: pool_command_sender.clone(),
            },
            bootstrap_state.pos,
            bootstrap_state.graph,
            bootstrap_state.compensation_millis,
        )
        .await
        .expect("could not start consensus controller");

    // launch bootstrap server
    let bootstrap_manager = start_bootstrap_server(
        consensus_command_sender.clone(),
        network_command_sender.clone(),
        execution_command_sender.clone(),
        &SETTINGS.bootstrap,
        massa_bootstrap::Establisher::new(),
        private_key,
        bootstrap_state.compensation_millis,
        *crate::settings::VERSION,
    )
    .await
    .unwrap();

    // spawn private API
    let (api_private, api_private_stop_rx) = API::<Private>::new(
        consensus_command_sender.clone(),
        network_command_sender.clone(),
        execution_command_sender.clone(),
        &SETTINGS.api,
        SETTINGS.consensus.config(),
    );
    let api_private_handle = api_private.serve(&SETTINGS.api.bind_private);

    // spawn public API
    let api_public = API::<Public>::new(
        consensus_command_sender.clone(),
        execution_command_sender,
        &SETTINGS.api,
        SETTINGS.consensus.config(),
        pool_command_sender.clone(),
        &SETTINGS.network,
        *crate::settings::VERSION,
        network_command_sender.clone(),
        bootstrap_state.compensation_millis,
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
        execution_manager,
        pool_manager,
        protocol_manager,
        network_manager,
        api_private_stop_rx,
        api_private_handle,
        api_public_handle,
    )
}

struct Managers {
    bootstrap_manager: Option<BootstrapManager>,
    consensus_manager: ConsensusManager,
    execution_manager: ExecutionManager,
    pool_manager: PoolManager,
    protocol_manager: ProtocolManager,
    network_manager: NetworkManager,
}

async fn stop(
    consensus_event_receiver: ConsensusEventReceiver,
    Managers {
        bootstrap_manager,
        consensus_manager,
        execution_manager,
        pool_manager,
        protocol_manager,
        network_manager,
    }: Managers,
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
    let (protocol_event_receiver, _execution_event_receiver) = consensus_manager
        .stop(consensus_event_receiver)
        .await
        .expect("consensus shutdown failed");

    // Stop execution controller.
    execution_manager
        .stop()
        .await
        .expect("Failed to shutdown execution.");

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
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_filter(match SETTINGS.logging.level {
            4 => LevelFilter::TRACE,
            3 => LevelFilter::DEBUG,
            2 => LevelFilter::INFO,
            1 => LevelFilter::WARN,
            _ => LevelFilter::ERROR,
        })
        .with_filter(filter_fn(|metadata| {
            metadata.target().starts_with("massa") // ignore non-massa logs
        }));
    tracing_subscriber::registry().with(fmt_layer).init();

    // run
    loop {
        let (
            _pool_command_sender,
            mut consensus_event_receiver,
            _consensus_command_sender,
            _network_command_sender,
            bootstrap_manager,
            consensus_manager,
            execution_manager,
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
            consensus_event_receiver,
            Managers {
                bootstrap_manager,
                consensus_manager,
                execution_manager,
                pool_manager,
                protocol_manager,
                network_manager,
            },
            api_private_handle,
            api_public_handle,
        )
        .await;

        if !restart {
            break;
        }
    }
}
