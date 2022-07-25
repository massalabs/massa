// Copyright (c) 2022 MASSA LABS <info@massa.net>

#![feature(ip)]
#![doc = include_str!("../../README.md")]
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
extern crate massa_logging;
use crate::settings::{POOL_CONFIG, SETTINGS};

use dialoguer::Password;
use massa_api::{Private, Public, RpcServer, StopHandle, API};
use massa_async_pool::AsyncPoolConfig;
use massa_bootstrap::{get_state, start_bootstrap_server, BootstrapManager};
use massa_cipher::{decrypt, encrypt};
use massa_consensus_exports::{
    events::ConsensusEvent, settings::ConsensusChannels, ConsensusCommandSender, ConsensusConfig,
    ConsensusEventReceiver, ConsensusManager,
};
use massa_consensus_worker::start_consensus_controller;
use massa_execution_exports::{ExecutionConfig, ExecutionManager};
use massa_execution_worker::start_execution_worker;
use massa_final_state::{FinalState, FinalStateConfig};
use massa_ledger_exports::LedgerConfig;
use massa_ledger_worker::FinalLedger;
use massa_logging::massa_trace;
use massa_models::{
    constants::{
        END_TIMESTAMP, GENESIS_TIMESTAMP, MAX_ASYNC_GAS, MAX_ASYNC_POOL_LENGTH, MAX_GAS_PER_BLOCK,
        OPERATION_VALIDITY_PERIODS, T0, THREAD_COUNT, VERSION,
    },
    init_serialization_context,
    prehash::Map,
    Address, SerializationContext,
};
use massa_network_exports::{Establisher, NetworkCommandSender, NetworkManager};
use massa_network_worker::start_network_controller;
use massa_pool::{start_pool_controller, PoolCommandSender, PoolManager};
use massa_protocol_exports::ProtocolManager;
use massa_protocol_worker::start_protocol_controller;
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_time::MassaTime;
use parking_lot::RwLock;
use std::{path::Path, process, sync::Arc};
use structopt::StructOpt;
use tokio::signal;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
#[cfg(not(feature = "instrument"))]
use tracing_subscriber::filter::{filter_fn, LevelFilter};

mod settings;

async fn launch(
    password: &str,
    staking_keys: &Map<Address, KeyPair>,
) -> (
    PoolCommandSender,
    ConsensusEventReceiver,
    ConsensusCommandSender,
    NetworkCommandSender,
    Option<BootstrapManager>,
    ConsensusManager,
    Box<dyn ExecutionManager>,
    PoolManager,
    ProtocolManager,
    NetworkManager,
    mpsc::Receiver<()>,
    StopHandle,
    StopHandle,
) {
    info!("Node version : {}", *VERSION);
    if let Some(end) = *END_TIMESTAMP {
        if MassaTime::now().expect("could not get now time") > end {
            panic!("This episode has come to an end, please get the latest testnet node version to continue");
        }
    }

    // Storage shared by multiple components.
    let shared_storage: Storage = Default::default();

    // init final state
    let ledger_config = LedgerConfig {
        initial_sce_ledger_path: SETTINGS.ledger.initial_sce_ledger_path.clone(),
        disk_ledger_path: SETTINGS.ledger.disk_ledger_path.clone(),
    };
    let async_pool_config = AsyncPoolConfig {
        max_length: MAX_ASYNC_POOL_LENGTH,
    };
    let final_state_config = FinalStateConfig {
        final_history_length: SETTINGS.ledger.final_history_length,
        thread_count: THREAD_COUNT,
        ledger_config: ledger_config.clone(),
        async_pool_config,
    };

    // Init the global serialization context
    init_serialization_context(SerializationContext::default());

    // Remove current disk ledger if there is one
    // NOTE: this is temporary, since we cannot currently handle bootstrap from remaining ledger
    if SETTINGS.ledger.disk_ledger_path.exists() {
        std::fs::remove_dir_all(SETTINGS.ledger.disk_ledger_path.clone())
            .expect("disk ledger delete failed");
    }

    // Create final ledger
    let ledger = FinalLedger::new(ledger_config.clone()).expect("could not init final ledger");

    // Create final state
    let final_state = Arc::new(RwLock::new(
        FinalState::new(final_state_config, Box::new(ledger)).expect("could not init final state"),
    ));

    // interrupt signal listener
    let stop_signal = signal::ctrl_c();
    tokio::pin!(stop_signal);

    // bootstrap
    let bootstrap_state = tokio::select! {
        _ = &mut stop_signal => {
            info!("interrupt signal received in bootstrap loop");
            process::exit(0);
        },
        res = get_state(
            &SETTINGS.bootstrap,
            final_state.clone(),
            massa_bootstrap::types::Establisher::new(),
            *VERSION,
            *GENESIS_TIMESTAMP,
            *END_TIMESTAMP,
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
            shared_storage.clone(),
            *VERSION,
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
        OPERATION_VALIDITY_PERIODS,
        MAX_GAS_PER_BLOCK,
        network_command_sender.clone(),
        network_event_receiver,
        shared_storage.clone(),
    )
    .await
    .expect("could not start protocol controller");

    // launch pool controller
    let (pool_command_sender, pool_manager) = start_pool_controller(
        &POOL_CONFIG,
        protocol_command_sender.clone(),
        protocol_pool_event_receiver,
        shared_storage.clone(),
    )
    .await
    .expect("could not start pool controller");

    // launch execution module
    let execution_config = ExecutionConfig {
        max_final_events: SETTINGS.execution.max_final_events,
        readonly_queue_length: SETTINGS.execution.readonly_queue_length,
        cursor_delay: SETTINGS.execution.cursor_delay,
        clock_compensation: bootstrap_state.compensation_millis,
        max_async_gas: MAX_ASYNC_GAS,
        thread_count: THREAD_COUNT,
        t0: T0,
        genesis_timestamp: *GENESIS_TIMESTAMP,
    };
    let (execution_manager, execution_controller) = start_execution_worker(
        execution_config,
        final_state.clone(),
        shared_storage.clone(),
    );

    // init consensus configuration
    let consensus_config = ConsensusConfig::from(&SETTINGS.consensus);
    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            consensus_config.clone(),
            ConsensusChannels {
                execution_controller: execution_controller.clone(),
                protocol_command_sender: protocol_command_sender.clone(),
                protocol_event_receiver,
                pool_command_sender: pool_command_sender.clone(),
            },
            bootstrap_state.pos,
            bootstrap_state.graph,
            shared_storage.clone(),
            bootstrap_state.compensation_millis,
            password.to_string(),
            staking_keys.to_owned(),
        )
        .await
        .expect("could not start consensus controller");

    // launch bootstrap server
    let bootstrap_manager = start_bootstrap_server(
        consensus_command_sender.clone(),
        network_command_sender.clone(),
        final_state.clone(),
        &SETTINGS.bootstrap,
        massa_bootstrap::Establisher::new(),
        private_key,
        bootstrap_state.compensation_millis,
        *VERSION,
    )
    .await
    .unwrap();

    // spawn private API
    let (api_private, api_private_stop_rx) = API::<Private>::new(
        consensus_command_sender.clone(),
        network_command_sender.clone(),
        execution_controller.clone(),
        &SETTINGS.api,
        consensus_config.clone(),
    );
    let api_private_handle = api_private.serve(&SETTINGS.api.bind_private);

    // spawn public API
    let api_public = API::<Public>::new(
        consensus_command_sender.clone(),
        execution_controller.clone(),
        &SETTINGS.api,
        consensus_config,
        pool_command_sender.clone(),
        &SETTINGS.network,
        *VERSION,
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
    execution_manager: Box<dyn ExecutionManager>,
    pool_manager: PoolManager,
    protocol_manager: ProtocolManager,
    network_manager: NetworkManager,
}

async fn stop(
    consensus_event_receiver: ConsensusEventReceiver,
    Managers {
        bootstrap_manager,
        consensus_manager,
        mut execution_manager,
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
    let protocol_event_receiver = consensus_manager
        .stop(consensus_event_receiver)
        .await
        .expect("consensus shutdown failed");

    // Stop execution controller.
    execution_manager.stop();

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

    // note that FinalLedger gets destroyed as soon as its Arc count goes to zero
}

#[derive(StructOpt)]
struct Args {
    /// Wallet password
    #[structopt(short = "p", long = "pwd")]
    password: Option<String>,
}

/// Ask for the staking keys file password and load them
async fn load_or_create_staking_keys_file(
    password: Option<String>,
    path: &Path,
) -> anyhow::Result<(String, Map<Address, KeyPair>)> {
    if path.is_file() {
        let password = password.unwrap_or_else(|| {
            Password::new()
                .with_prompt("Enter staking keys file password")
                .interact()
                .expect("IO error: Password reading failed, staking keys file couldn't be unlocked")
        });
        let (_version, decrypted_data) = decrypt(&password, &tokio::fs::read(path).await?)?;
        let staking_keys: anyhow::Result<Map<Address, KeyPair>> = Ok(serde_json::from_slice::<
            Map<Address, KeyPair>,
        >(&decrypted_data)?);
        Ok((password, staking_keys?))
    } else {
        let password = password.unwrap_or_else(|| {
            Password::new()
                .with_prompt("Enter new password for staking keys file")
                .with_confirmation("Confirm password", "Passwords mismatching")
                .interact()
                .expect("IO error: Password reading failed, staking keys file couldn't be created")
        });
        let json = serde_json::to_string_pretty(&Map::<Address, KeyPair>::default())?;
        let encrypted_data = encrypt(&password, json.as_bytes())?;
        tokio::fs::write(path, encrypted_data).await?;
        Ok((password, Map::default()))
    }
}

/// To instrument `massa-node` with `tokio-console` run
/// ```shell
/// RUSTFLAGS="--cfg tokio_unstable" cargo run --bin massa-node --features instrument
/// ```
#[paw::main]
#[tokio::main]
async fn main(args: Args) -> anyhow::Result<()> {
    use tracing_subscriber::prelude::*;
    // spawn the console server in the background, returning a `Layer`:
    #[cfg(feature = "instrument")]
    let tracing_layer = console_subscriber::spawn();
    #[cfg(not(feature = "instrument"))]
    let tracing_layer = tracing_subscriber::fmt::layer()
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
    // build a `Subscriber` by combining layers with a `tracing_subscriber::Registry`:
    tracing_subscriber::registry()
        // add the console layer to the subscriber or default layers...
        .with(tracing_layer)
        .init();

    // Setup panic handlers,
    // and when a panic occurs,
    // run default handler,
    // and then shutdown.
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    // run
    let (password, staking_keys) =
        load_or_create_staking_keys_file(args.password, &SETTINGS.consensus.staking_keys_path)
            .await?;
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
        ) = launch(&password, &staking_keys).await;

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
    Ok(())
}
