// Copyright (c) 2022 MASSA LABS <info@massa.net>

#![doc = include_str!("../../README.md")]
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
extern crate massa_logging;
use crate::settings::SETTINGS;

use dialoguer::Password;
use massa_api::{APIConfig, Private, Public, RpcServer, StopHandle, API};
use massa_async_pool::AsyncPoolConfig;
use massa_bootstrap::{get_state, start_bootstrap_server, BootstrapConfig, BootstrapManager};
use massa_consensus_exports::{
    events::ConsensusEvent, settings::ConsensusChannels, ConsensusConfig, ConsensusEventReceiver,
    ConsensusManager,
};
use massa_consensus_worker::start_consensus_controller;
use massa_execution_exports::{ExecutionConfig, ExecutionManager};
use massa_execution_worker::start_execution_worker;
use massa_factory_exports::{FactoryChannels, FactoryConfig, FactoryManager};
use massa_factory_worker::start_factory;
use massa_final_state::{FinalState, FinalStateConfig};
use massa_ledger_exports::LedgerConfig;
use massa_ledger_worker::FinalLedger;
use massa_logging::massa_trace;
use massa_models::constants::default::{
    ADDRESS_SIZE_BYTES, BLOCK_REWARD, BOOTSTRAP_RANDOMNESS_SIZE_BYTES, ENDORSEMENT_COUNT,
    END_TIMESTAMP, GENESIS_KEY, GENESIS_TIMESTAMP, INITIAL_DRAW_SEED,
    LEDGER_PART_SIZE_MESSAGE_BYTES, MAX_ADVERTISE_LENGTH, MAX_ASK_BLOCKS_PER_MESSAGE,
    MAX_ASYNC_GAS, MAX_ASYNC_POOL_LENGTH, MAX_BLOCK_SIZE, MAX_BOOTSTRAP_ASYNC_POOL_CHANGES,
    MAX_BOOTSTRAP_BLOCKS, MAX_BOOTSTRAP_ERROR_LENGTH, MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE,
    MAX_BOOTSTRAP_MESSAGE_SIZE, MAX_DATASTORE_ENTRY_COUNT, MAX_DATASTORE_KEY_LENGTH,
    MAX_DATASTORE_VALUE_LENGTH, MAX_DATA_ASYNC_MESSAGE, MAX_ENDORSEMENTS_PER_MESSAGE,
    MAX_FUNCTION_NAME_LENGTH, MAX_GAS_PER_BLOCK, MAX_LEDGER_CHANGES_COUNT, MAX_MESSAGE_SIZE,
    MAX_OPERATIONS_PER_BLOCK, MAX_PARAMETERS_SIZE, OPERATION_VALIDITY_PERIODS, PERIODS_PER_CYCLE,
    ROLL_PRICE, T0, THREAD_COUNT, VERSION,
};
use massa_models::Address;
use massa_network_exports::{Establisher, NetworkConfig, NetworkManager};
use massa_network_worker::start_network_controller;
use massa_pool_exports::{PoolConfig, PoolController};
use massa_pool_worker::start_pool;
use massa_pos_exports::{SelectorConfig, SelectorManager};
use massa_pos_worker::start_selector_worker;
use massa_protocol_exports::ProtocolManager;
use massa_protocol_worker::start_protocol_controller;
use massa_storage::Storage;
use massa_time::MassaTime;
use massa_wallet::Wallet;
use std::{mem, path::PathBuf, sync::RwLock};
use std::{path::Path, process, sync::Arc};
use structopt::StructOpt;
use tokio::signal;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
#[cfg(not(feature = "instrument"))]
use tracing_subscriber::filter::{filter_fn, LevelFilter};

mod settings;

async fn launch(
    node_wallet: Arc<RwLock<Wallet>>,
) -> (
    ConsensusEventReceiver,
    Option<BootstrapManager>,
    ConsensusManager,
    Box<dyn ExecutionManager>,
    Box<dyn SelectorManager>,
    Box<dyn PoolController>,
    ProtocolManager,
    NetworkManager,
    Box<dyn FactoryManager>,
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
        max_key_length: MAX_DATASTORE_KEY_LENGTH,
        max_ledger_part_size: LEDGER_PART_SIZE_MESSAGE_BYTES,
        address_bytes_size: ADDRESS_SIZE_BYTES,
    };
    let async_pool_config = AsyncPoolConfig {
        max_length: MAX_ASYNC_POOL_LENGTH,
    };
    let final_state_config = FinalStateConfig {
        final_history_length: SETTINGS.ledger.final_history_length,
        thread_count: THREAD_COUNT,
        ledger_config: ledger_config.clone(),
        periods_per_cycle: PERIODS_PER_CYCLE,
        initial_seed_string: INITIAL_DRAW_SEED.into(),
        initial_rolls_path: SETTINGS.consensus.initial_rolls_path.clone(),
        async_pool_config,
    };

    // Remove current disk ledger if there is one
    // NOTE: this is temporary, since we cannot currently handle bootstrap from remaining ledger
    if SETTINGS.ledger.disk_ledger_path.exists() {
        std::fs::remove_dir_all(SETTINGS.ledger.disk_ledger_path.clone())
            .expect("disk ledger delete failed");
    }

    // Create final ledger
    let ledger = FinalLedger::new(ledger_config.clone()).expect("could not init final ledger");

    // Create final state
    let final_state = Arc::new(parking_lot::RwLock::new(
        FinalState::new(final_state_config, Box::new(ledger)).expect("could not init final state"),
    ));

    // interrupt signal listener
    let stop_signal = signal::ctrl_c();
    tokio::pin!(stop_signal);

    let bootstrap_config: BootstrapConfig = BootstrapConfig {
        bootstrap_list: SETTINGS.bootstrap.bootstrap_list.clone(),
        bind: SETTINGS.bootstrap.bind,
        connect_timeout: SETTINGS.bootstrap.connect_timeout,
        read_timeout: SETTINGS.bootstrap.read_timeout,
        write_timeout: SETTINGS.bootstrap.write_timeout,
        read_error_timeout: SETTINGS.bootstrap.read_error_timeout,
        write_error_timeout: SETTINGS.bootstrap.write_error_timeout,
        retry_delay: SETTINGS.bootstrap.retry_delay,
        max_ping: SETTINGS.bootstrap.max_ping,
        enable_clock_synchronization: SETTINGS.bootstrap.enable_clock_synchronization,
        cache_duration: SETTINGS.bootstrap.cache_duration,
        max_simultaneous_bootstraps: SETTINGS.bootstrap.max_simultaneous_bootstraps,
        per_ip_min_interval: SETTINGS.bootstrap.per_ip_min_interval,
        ip_list_max_size: SETTINGS.bootstrap.ip_list_max_size,
        max_bytes_read_write: SETTINGS.bootstrap.max_bytes_read_write,
        max_bootstrap_message_size: MAX_BOOTSTRAP_MESSAGE_SIZE,
        max_datastore_key_length: MAX_DATASTORE_KEY_LENGTH,
        randomness_size_bytes: BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
        thread_count: THREAD_COUNT,
        endorsement_count: ENDORSEMENT_COUNT,
        max_advertise_length: MAX_ADVERTISE_LENGTH,
        max_bootstrap_async_pool_changes: MAX_BOOTSTRAP_ASYNC_POOL_CHANGES,
        max_bootstrap_blocks_length: MAX_BOOTSTRAP_BLOCKS,
        max_bootstrap_error_length: MAX_BOOTSTRAP_ERROR_LENGTH,
        max_bootstrap_final_state_parts_size: MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE,
        max_data_async_message: MAX_DATA_ASYNC_MESSAGE,
        max_operations_per_blocks: MAX_OPERATIONS_PER_BLOCK,
        max_datastore_entry_count: MAX_DATASTORE_ENTRY_COUNT,
        max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
        max_function_name_length: MAX_FUNCTION_NAME_LENGTH,
        max_ledger_changes_count: MAX_LEDGER_CHANGES_COUNT,
        max_parameters_size: MAX_PARAMETERS_SIZE,
    };

    // bootstrap
    let bootstrap_state = tokio::select! {
        _ = &mut stop_signal => {
            info!("interrupt signal received in bootstrap loop");
            process::exit(0);
        },
        res = get_state(
            &bootstrap_config,
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

    let network_config: NetworkConfig = NetworkConfig {
        bind: SETTINGS.network.bind,
        routable_ip: SETTINGS.network.routable_ip,
        protocol_port: SETTINGS.network.protocol_port,
        connect_timeout: SETTINGS.network.connect_timeout,
        wakeup_interval: SETTINGS.network.wakeup_interval,
        initial_peers_file: SETTINGS.network.initial_peers_file.clone(),
        peers_file: SETTINGS.network.peers_file.clone(),
        keypair_file: SETTINGS.network.keypair_file.clone(),
        peer_types_config: SETTINGS.network.peer_types_config.clone(),
        max_in_connections_per_ip: SETTINGS.network.max_in_connections_per_ip,
        max_idle_peers: SETTINGS.network.max_idle_peers,
        max_banned_peers: SETTINGS.network.max_banned_peers,
        peers_file_dump_interval: SETTINGS.network.peers_file_dump_interval,
        message_timeout: SETTINGS.network.message_timeout,
        ask_peer_list_interval: SETTINGS.network.ask_peer_list_interval,
        max_send_wait: SETTINGS.network.max_send_wait,
        ban_timeout: SETTINGS.network.ban_timeout,
        peer_list_send_timeout: SETTINGS.network.peer_list_send_timeout,
        max_in_connection_overflow: SETTINGS.network.max_in_connection_overflow,
        max_operations_per_message: SETTINGS.network.max_operations_per_message,
        max_bytes_read: SETTINGS.network.max_bytes_read,
        max_bytes_write: SETTINGS.network.max_bytes_write,
        max_ask_blocks: MAX_ASK_BLOCKS_PER_MESSAGE,
        max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
        thread_count: THREAD_COUNT,
        endorsement_count: ENDORSEMENT_COUNT,
        max_peer_advertise_length: MAX_ADVERTISE_LENGTH,
        max_endorsements_per_message: MAX_ENDORSEMENTS_PER_MESSAGE,
        max_message_size: MAX_MESSAGE_SIZE,
        max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
        max_function_name_length: MAX_FUNCTION_NAME_LENGTH,
        max_parameters_size: MAX_PARAMETERS_SIZE,
    };

    // launch network controller
    let (network_command_sender, network_event_receiver, network_manager, private_key, node_id) =
        start_network_controller(
            &network_config, // TODO: get rid of this clone() ... see #1277
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
        _protocol_pool_event_receiver,
        protocol_manager,
    ) = start_protocol_controller(
        SETTINGS.protocol.into(),
        network_command_sender.clone(),
        network_event_receiver,
        shared_storage.clone(),
    )
    .await
    .expect("could not start protocol controller");

    // launch selector worker
    let (selector_manager, selector_controller) = start_selector_worker(SelectorConfig {
        max_draw_cache: SETTINGS.selector.max_draw_cache,
        channel_size: SETTINGS.selector.channel_size,
        thread_count: THREAD_COUNT,
        endorsement_count: ENDORSEMENT_COUNT,
        periods_per_cycle: PERIODS_PER_CYCLE,
        genesis_address: Address::from_public_key(&GENESIS_KEY.get_public_key()),
        initial_rolls_path: SETTINGS.consensus.initial_rolls_path.clone(),
        initial_draw_seed: INITIAL_DRAW_SEED.into(),
    })
    .expect("could not start selector worker");

    // give the controller to final state in order for it to feed the cycles
    final_state
        .write()
        .give_selector_controller(selector_controller.clone())
        .expect("could give selector controller to final state"); // TODO: this might just mean a bad bootstrap, no need to panic, just reboot

    // launch execution module
    let execution_config = ExecutionConfig {
        max_final_events: SETTINGS.execution.max_final_events,
        readonly_queue_length: SETTINGS.execution.readonly_queue_length,
        cursor_delay: SETTINGS.execution.cursor_delay,
        clock_compensation: bootstrap_state.compensation_millis,
        max_async_gas: MAX_ASYNC_GAS,
        max_gas_per_block: MAX_GAS_PER_BLOCK,
        roll_price: ROLL_PRICE,
        thread_count: THREAD_COUNT,
        t0: T0,
        genesis_timestamp: *GENESIS_TIMESTAMP,
        block_reward: BLOCK_REWARD,
        endorsement_count: ENDORSEMENT_COUNT as u64,
        operation_validity_period: OPERATION_VALIDITY_PERIODS,
        periods_per_cycle: PERIODS_PER_CYCLE,
    };
    let (execution_manager, execution_controller) = start_execution_worker(
        execution_config,
        final_state.clone(),
        selector_controller.clone(),
    );

    // launch pool controller
    let pool_config = PoolConfig {
        thread_count: THREAD_COUNT,
        max_block_size: MAX_BLOCK_SIZE,
        max_block_gas: MAX_GAS_PER_BLOCK,
        roll_price: ROLL_PRICE,
        max_block_endorsement_count: ENDORSEMENT_COUNT,
        operation_validity_periods: OPERATION_VALIDITY_PERIODS,
        max_operation_pool_size_per_thread: SETTINGS.pool.max_pool_size_per_thread,
        max_endorsements_pool_size_per_thread: SETTINGS.pool.max_pool_size_per_thread,
    };
    let pool_controller = start_pool(pool_config, &shared_storage, execution_controller.clone());
    let pool_manager: Box<dyn PoolController> = Box::new(pool_controller.clone());

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
                pool_command_sender: pool_manager.clone(),
                selector_controller: selector_controller.clone(),
            },
            bootstrap_state.graph,
            shared_storage.clone(),
            bootstrap_state.compensation_millis,
        )
        .await
        .expect("could not start consensus controller");

    // launch factory
    let factory_config = FactoryConfig {
        thread_count: THREAD_COUNT,
        genesis_timestamp: *GENESIS_TIMESTAMP,
        t0: T0,
        clock_compensation_millis: bootstrap_state.compensation_millis,
        initial_delay: SETTINGS.factory.initial_delay,
        max_block_size: MAX_BLOCK_SIZE as u64,
        max_block_gas: MAX_GAS_PER_BLOCK,
    };
    let factory_channels = FactoryChannels {
        selector: selector_controller.clone(),
        consensus: consensus_command_sender.clone(),
        pool: pool_manager.clone(),
        storage: shared_storage.clone(),
    };
    let factory_manager = start_factory(factory_config, node_wallet.clone(), factory_channels);

    // launch bootstrap server
    let bootstrap_manager = start_bootstrap_server(
        consensus_command_sender.clone(),
        network_command_sender.clone(),
        final_state.clone(),
        bootstrap_config,
        massa_bootstrap::Establisher::new(),
        private_key,
        bootstrap_state.compensation_millis,
        *VERSION,
    )
    .await
    .unwrap();

    let api_config: APIConfig = APIConfig {
        bind_private: SETTINGS.api.bind_private,
        bind_public: SETTINGS.api.bind_public,
        draw_lookahead_period_count: SETTINGS.api.draw_lookahead_period_count,
        max_arguments: SETTINGS.api.max_arguments,
        max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
        max_function_name_length: MAX_FUNCTION_NAME_LENGTH,
        max_parameter_size: MAX_PARAMETERS_SIZE,
    };
    // spawn private API
    let (api_private, api_private_stop_rx) = API::<Private>::new(
        consensus_command_sender.clone(),
        network_command_sender.clone(),
        execution_controller.clone(),
        api_config.clone(),
        consensus_config.clone(),
        node_wallet,
    );
    let api_private_handle = api_private.serve(&SETTINGS.api.bind_private);

    // spawn public API
    let api_public = API::<Public>::new(
        consensus_command_sender.clone(),
        execution_controller.clone(),
        api_config.clone(),
        selector_controller.clone(),
        consensus_config,
        pool_manager.clone(),
        network_config,
        *VERSION,
        network_command_sender.clone(),
        bootstrap_state.compensation_millis,
        node_id,
        shared_storage.clone(),
    );
    let api_public_handle = api_public.serve(&SETTINGS.api.bind_public);

    (
        consensus_event_receiver,
        bootstrap_manager,
        consensus_manager,
        execution_manager,
        selector_manager,
        pool_manager,
        protocol_manager,
        network_manager,
        factory_manager,
        api_private_stop_rx,
        api_private_handle,
        api_public_handle,
    )
}

struct Managers {
    bootstrap_manager: Option<BootstrapManager>,
    consensus_manager: ConsensusManager,
    execution_manager: Box<dyn ExecutionManager>,
    selector_manager: Box<dyn SelectorManager>,
    pool_manager: Box<dyn PoolController>,
    protocol_manager: ProtocolManager,
    network_manager: NetworkManager,
    factory_manager: Box<dyn FactoryManager>,
}

async fn stop(
    consensus_event_receiver: ConsensusEventReceiver,
    Managers {
        bootstrap_manager,
        consensus_manager,
        mut execution_manager,
        mut selector_manager,
        pool_manager,
        protocol_manager,
        network_manager,
        mut factory_manager,
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

    // stop factory
    factory_manager.stop();

    // stop consensus controller
    let protocol_event_receiver = consensus_manager
        .stop(consensus_event_receiver)
        .await
        .expect("consensus shutdown failed");

    // stop pool
    //TODO make a proper manager
    mem::drop(pool_manager);

    // stop execution controller
    execution_manager.stop();

    // stop selector controller
    selector_manager.stop();

    // stop pool controller
    // TODO
    //let protocol_pool_event_receiver = pool_manager.stop().await.expect("pool shutdown failed");

    // stop protocol controller
    let network_event_receiver = protocol_manager
        .stop(
            protocol_event_receiver, /*, protocol_pool_event_receiver*/
        )
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

/// Load wallet, asking for passwords if necessary
fn load_wallet(password: Option<String>, path: &Path) -> anyhow::Result<Arc<RwLock<Wallet>>> {
    let password = if path.is_file() {
        password.unwrap_or_else(|| {
            Password::new()
                .with_prompt("Enter staking keys file password")
                .interact()
                .expect("IO error: Password reading failed, staking keys file couldn't be unlocked")
        })
    } else {
        password.unwrap_or_else(|| {
            Password::new()
                .with_prompt("Enter new password for staking keys file")
                .with_confirmation("Confirm password", "Passwords mismatching")
                .interact()
                .expect("IO error: Password reading failed, staking keys file couldn't be created")
        })
    };
    Ok(Arc::new(RwLock::new(Wallet::new(
        PathBuf::from(path),
        password,
    )?)))
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

    // load or create wallet, asking for password if necessary

    let node_wallet = load_wallet(args.password, &SETTINGS.consensus.staking_keys_path)?;

    loop {
        let (
            mut consensus_event_receiver,
            bootstrap_manager,
            consensus_manager,
            execution_manager,
            selector_manager,
            pool_manager,
            protocol_manager,
            network_manager,
            factory_manager,
            mut api_private_stop_rx,
            api_private_handle,
            api_public_handle,
        ) = launch(node_wallet.clone()).await;

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
                selector_manager,
                pool_manager,
                protocol_manager,
                network_manager,
                factory_manager,
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
