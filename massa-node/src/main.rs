// Copyright (c) 2021 MASSA LABS <info@massa.net>

#![feature(ip)]
#![feature(destructuring_assignment)]
#![doc = include_str!("../../README.md")]

extern crate logging;

use api::{ApiMassaPrivate, ApiMassaPrivateStopHandle, ApiMassaPublic, ApiMassaPublicStopHandle};
use bootstrap::{get_state, start_bootstrap_server, BootstrapManager};
use consensus::{
    start_consensus_controller, ConsensusCommandSender, ConsensusEvent, ConsensusEventReceiver,
    ConsensusManager,
};
use logging::massa_trace;
use models::{init_serialization_context, SerializationContext};
use network::{start_network_controller, Establisher, NetworkCommandSender, NetworkManager};
use pool::{start_pool_controller, PoolCommandSender, PoolManager};
use protocol::{start_protocol_controller, ProtocolManager};
use storage::{start_storage, StorageManager};
use time::UTime;
use tokio::signal;
use tokio::sync::mpsc;
use tracing::{error, info, warn, Level};

mod node_config;

async fn launch(
    cfg: node_config::Config,
) -> (
    PoolCommandSender,
    ConsensusEventReceiver,
    ConsensusCommandSender,
    NetworkCommandSender,
    Option<BootstrapManager>,
    ConsensusManager,
    PoolManager,
    ProtocolManager,
    StorageManager,
    NetworkManager,
    mpsc::Receiver<()>,
    ApiMassaPrivateStopHandle,
    ApiMassaPublicStopHandle,
) {
    info!("Node version : {}", cfg.version);
    if let Some(end) = cfg.consensus.end_timestamp {
        if UTime::now(0).expect("could not get now time") > end {
            panic!("This episode has come to an end, please get the latest testnet node version to continue");
        }
    }

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
        max_endorsements_per_message: cfg.network.max_endorsements_per_message,
        max_bootstrap_message_size: cfg.bootstrap.max_bootstrap_message_size,
        max_bootstrap_pos_cycles: cfg.bootstrap.max_bootstrap_pos_cycles,
        max_bootstrap_pos_entries: cfg.bootstrap.max_bootstrap_pos_entries,
        max_block_endorsments: cfg.consensus.endorsement_count,
    });

    let (boot_pos, boot_graph, clock_compensation, initial_peers) = get_state(
        cfg.bootstrap.clone(),
        bootstrap::establisher::Establisher::new(),
        cfg.version,
        cfg.consensus.genesis_timestamp,
        cfg.consensus.end_timestamp,
    )
    .await
    .unwrap();

    // launch network controller
    let (network_command_sender, network_event_receiver, network_manager, private_key, node_id) =
        start_network_controller(
            cfg.network.clone(),
            Establisher::new(),
            clock_compensation,
            initial_peers,
            cfg.version,
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
        cfg.consensus.operation_validity_periods,
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
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
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

    // launch bootstrap server
    let bootstrap_manager = start_bootstrap_server(
        consensus_command_sender.clone(),
        network_command_sender.clone(),
        cfg.bootstrap,
        bootstrap::Establisher::new(),
        private_key,
        clock_compensation,
        cfg.version,
    )
    .await
    .unwrap();

    // spawn private API
    let (api_private, api_private_stop_rx) = ApiMassaPrivate::create(
        &cfg.new_api.bind_private.to_string(),
        consensus_command_sender.clone(),
        network_command_sender.clone(),
        cfg.new_api.clone(),
        cfg.consensus.clone(),
    );
    let api_private_handle = api_private.serve_massa_private();

    // spawn public API
    let api_public = ApiMassaPublic::create(
        &cfg.new_api.bind_public.to_string(),
        consensus_command_sender.clone(),
        cfg.new_api,
        cfg.consensus,
        pool_command_sender.clone(),
        Some(storage_command_sender),
        cfg.network,
        cfg.version,
        network_command_sender.clone(),
        clock_compensation,
        node_id,
    );
    let api_public_handle = api_public.serve_massa_public();

    (
        pool_command_sender,
        consensus_event_receiver,
        consensus_command_sender,
        network_command_sender,
        bootstrap_manager,
        consensus_manager,
        pool_manager,
        protocol_manager,
        storage_manager,
        network_manager,
        api_private_stop_rx,
        api_private_handle,
        api_public_handle,
    )
}

// TODO: IDEA identify it unreachable code?
async fn run(cfg: node_config::Config) {
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
            storage_manager,
            network_manager,
            mut api_private_stop_rx,
            api_private_handle,
            api_public_handle,
        ) = launch(cfg.clone()).await;

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
                            break false ;
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
            storage_manager,
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

async fn stop(
    bootstrap_manager: Option<BootstrapManager>,
    consensus_manager: ConsensusManager,
    consensus_event_receiver: ConsensusEventReceiver,
    pool_manager: PoolManager,
    protocol_manager: ProtocolManager,
    storage_manager: StorageManager,
    network_manager: NetworkManager,
    api_private_handle: ApiMassaPrivateStopHandle,
    api_public_handle: ApiMassaPublicStopHandle,
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

    // stop storage controller
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
    let config_path = "base_config/config.toml";
    let override_config_path = "config/config.toml";
    let mut cfg = config::Config::default();
    cfg.merge(config::File::with_name(config_path))
        .expect("could not load main config file");
    if std::path::Path::new(override_config_path).is_file() {
        cfg.merge(config::File::with_name(override_config_path))
            .expect("could not load override config file");
    }
    let cfg = cfg
        .try_into::<node_config::Config>()
        .expect("error structuring config");

    // setup logging
    tracing_subscriber::fmt()
        .with_max_level(match cfg.logging.level {
            4 => Level::TRACE,
            3 => Level::DEBUG,
            2 => Level::INFO,
            1 => Level::WARN,
            _ => Level::ERROR,
        })
        .init();

    run(cfg).await
}
