// Copyright (c) 2022 MASSA LABS <info@massa.net>

#![doc = include_str!("../../README.md")]
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
extern crate massa_logging;
use crate::settings::SETTINGS;

use crossbeam_channel::{Receiver, TryRecvError};
use dialoguer::Password;
use massa_api::{ApiServer, ApiV2, Private, Public, RpcServer, StopHandle, API};
use massa_api_exports::config::APIConfig;
use massa_async_pool::AsyncPoolConfig;
use massa_bootstrap::{
    get_state, start_bootstrap_server, BSEstablisher, BootstrapConfig, BootstrapManager,
};
use massa_consensus_exports::events::ConsensusEvent;
use massa_consensus_exports::{ConsensusChannels, ConsensusConfig, ConsensusManager};
use massa_consensus_worker::start_consensus_worker;
use massa_executed_ops::ExecutedOpsConfig;
use massa_execution_exports::{ExecutionConfig, ExecutionManager, GasCosts, StorageCostsConstants};
use massa_execution_worker::start_execution_worker;
use massa_factory_exports::{FactoryChannels, FactoryConfig, FactoryManager};
use massa_factory_worker::start_factory;
use massa_final_state::{FinalState, FinalStateConfig};
use massa_ledger_exports::LedgerConfig;
use massa_ledger_worker::FinalLedger;
use massa_logging::massa_trace;
use massa_models::address::Address;
use massa_models::config::constants::{
    ASYNC_POOL_BOOTSTRAP_PART_SIZE, BLOCK_REWARD, BOOTSTRAP_RANDOMNESS_SIZE_BYTES, CHANNEL_SIZE,
    DEFERRED_CREDITS_BOOTSTRAP_PART_SIZE, DELTA_F0, ENDORSEMENT_COUNT, END_TIMESTAMP,
    EXECUTED_OPS_BOOTSTRAP_PART_SIZE, GENESIS_KEY, GENESIS_TIMESTAMP, INITIAL_DRAW_SEED,
    LEDGER_COST_PER_BYTE, LEDGER_ENTRY_BASE_SIZE, LEDGER_ENTRY_DATASTORE_BASE_SIZE,
    LEDGER_PART_SIZE_MESSAGE_BYTES, MAX_ADVERTISE_LENGTH, MAX_ASK_BLOCKS_PER_MESSAGE,
    MAX_ASYNC_GAS, MAX_ASYNC_MESSAGE_DATA, MAX_ASYNC_POOL_LENGTH, MAX_BLOCK_SIZE,
    MAX_BOOTSTRAP_ASYNC_POOL_CHANGES, MAX_BOOTSTRAP_BLOCKS, MAX_BOOTSTRAP_ERROR_LENGTH,
    MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE, MAX_BOOTSTRAP_MESSAGE_SIZE, MAX_BYTECODE_LENGTH,
    MAX_CONSENSUS_BLOCKS_IDS, MAX_DATASTORE_ENTRY_COUNT, MAX_DATASTORE_KEY_LENGTH,
    MAX_DATASTORE_VALUE_LENGTH, MAX_DEFERRED_CREDITS_LENGTH, MAX_ENDORSEMENTS_PER_MESSAGE,
    MAX_EXECUTED_OPS_CHANGES_LENGTH, MAX_EXECUTED_OPS_LENGTH, MAX_FUNCTION_NAME_LENGTH,
    MAX_GAS_PER_BLOCK, MAX_LEDGER_CHANGES_COUNT, MAX_MESSAGE_SIZE, MAX_OPERATIONS_PER_BLOCK,
    MAX_OPERATION_DATASTORE_ENTRY_COUNT, MAX_OPERATION_DATASTORE_KEY_LENGTH,
    MAX_OPERATION_DATASTORE_VALUE_LENGTH, MAX_PARAMETERS_SIZE, MAX_PRODUCTION_STATS_LENGTH,
    MAX_ROLLS_COUNT_LENGTH, NETWORK_CONTROLLER_CHANNEL_SIZE, NETWORK_EVENT_CHANNEL_SIZE,
    NETWORK_NODE_COMMAND_CHANNEL_SIZE, NETWORK_NODE_EVENT_CHANNEL_SIZE, OPERATION_VALIDITY_PERIODS,
    PERIODS_PER_CYCLE, POOL_CONTROLLER_CHANNEL_SIZE, POS_MISS_RATE_DEACTIVATION_THRESHOLD,
    POS_SAVED_CYCLES, PROTOCOL_CONTROLLER_CHANNEL_SIZE, PROTOCOL_EVENT_CHANNEL_SIZE, ROLL_PRICE,
    T0, THREAD_COUNT, VERSION,
};
use massa_models::config::CONSENSUS_BOOTSTRAP_PART_SIZE;
use massa_network_exports::{Establisher, NetworkConfig, NetworkManager};
use massa_network_worker::start_network_controller;
use massa_pool_exports::{PoolChannels, PoolConfig, PoolManager};
use massa_pool_worker::start_pool_controller;
use massa_pos_exports::{PoSConfig, SelectorConfig, SelectorManager};
use massa_pos_worker::start_selector_worker;
use massa_protocol_exports::{
    ProtocolCommand, ProtocolCommandSender, ProtocolConfig, ProtocolManager, ProtocolReceivers,
    ProtocolSenders,
};
use massa_protocol_worker::start_protocol_controller;
use massa_storage::Storage;
use massa_time::MassaTime;
use massa_wallet::Wallet;
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::sleep;
use std::time::Duration;
use std::{path::Path, process, sync::Arc};
use structopt::StructOpt;
use tokio::net::TcpStream;
use tokio::signal;
use tokio::sync::{broadcast, mpsc};
use tracing::{error, info, warn};
use tracing_subscriber::filter::{filter_fn, LevelFilter};

mod settings;

async fn launch(
    _args: &Args,
    node_wallet: Arc<RwLock<Wallet>>,
) -> (
    Receiver<ConsensusEvent>,
    Option<BootstrapManager<TcpStream>>,
    Box<dyn ConsensusManager>,
    Box<dyn ExecutionManager>,
    Box<dyn SelectorManager>,
    Box<dyn PoolManager>,
    ProtocolManager,
    NetworkManager,
    Box<dyn FactoryManager>,
    mpsc::Receiver<()>,
    StopHandle,
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
    let shared_storage: Storage = Storage::create_root();

    // init final state
    let ledger_config = LedgerConfig {
        thread_count: THREAD_COUNT,
        initial_ledger_path: SETTINGS.ledger.initial_ledger_path.clone(),
        disk_ledger_path: SETTINGS.ledger.disk_ledger_path.clone(),
        max_key_length: MAX_DATASTORE_KEY_LENGTH,
        max_ledger_part_size: LEDGER_PART_SIZE_MESSAGE_BYTES,
        max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
    };
    let async_pool_config = AsyncPoolConfig {
        max_length: MAX_ASYNC_POOL_LENGTH,
        thread_count: THREAD_COUNT,
        bootstrap_part_size: ASYNC_POOL_BOOTSTRAP_PART_SIZE,
        max_async_message_data: MAX_ASYNC_MESSAGE_DATA,
    };
    let pos_config = PoSConfig {
        periods_per_cycle: PERIODS_PER_CYCLE,
        thread_count: THREAD_COUNT,
        cycle_history_length: POS_SAVED_CYCLES,
        credits_bootstrap_part_size: DEFERRED_CREDITS_BOOTSTRAP_PART_SIZE,
    };
    let executed_ops_config = ExecutedOpsConfig {
        thread_count: THREAD_COUNT,
        bootstrap_part_size: EXECUTED_OPS_BOOTSTRAP_PART_SIZE,
    };
    let final_state_config = FinalStateConfig {
        ledger_config: ledger_config.clone(),
        async_pool_config,
        pos_config,
        executed_ops_config,
        final_history_length: SETTINGS.ledger.final_history_length,
        thread_count: THREAD_COUNT,
        periods_per_cycle: PERIODS_PER_CYCLE,
        initial_seed_string: INITIAL_DRAW_SEED.into(),
        initial_rolls_path: SETTINGS.selector.initial_rolls_path.clone(),
    };

    // Remove current disk ledger if there is one
    // NOTE: this is temporary, since we cannot currently handle bootstrap from remaining ledger
    if SETTINGS.ledger.disk_ledger_path.exists() {
        std::fs::remove_dir_all(SETTINGS.ledger.disk_ledger_path.clone())
            .expect("disk ledger delete failed");
    }

    // Create final ledger
    let ledger = FinalLedger::new(ledger_config.clone());

    // launch selector worker
    let (selector_manager, selector_controller) = start_selector_worker(SelectorConfig {
        max_draw_cache: SETTINGS.selector.max_draw_cache,
        channel_size: CHANNEL_SIZE,
        thread_count: THREAD_COUNT,
        endorsement_count: ENDORSEMENT_COUNT,
        periods_per_cycle: PERIODS_PER_CYCLE,
        genesis_address: Address::from_public_key(&GENESIS_KEY.get_public_key()),
    })
    .expect("could not start selector worker");

    // Create final state
    let final_state = Arc::new(parking_lot::RwLock::new(
        FinalState::new(
            final_state_config,
            Box::new(ledger),
            selector_controller.clone(),
        )
        .expect("could not init final state"),
    ));

    // interrupt signal listener
    let stop_signal = signal::ctrl_c();
    tokio::pin!(stop_signal);

    let bootstrap_config: BootstrapConfig = BootstrapConfig {
        bootstrap_list: SETTINGS.bootstrap.bootstrap_list.clone(),
        bootstrap_protocol: SETTINGS.bootstrap.bootstrap_protocol,
        bootstrap_whitelist_path: SETTINGS.bootstrap.bootstrap_whitelist_path.clone(),
        bootstrap_blacklist_path: SETTINGS.bootstrap.bootstrap_blacklist_path.clone(),
        listen_addr: SETTINGS.bootstrap.bind,
        connect_timeout: SETTINGS.bootstrap.connect_timeout,
        bootstrap_timeout: SETTINGS.bootstrap.bootstrap_timeout,
        read_timeout: SETTINGS.bootstrap.read_timeout,
        write_timeout: SETTINGS.bootstrap.write_timeout,
        read_error_timeout: SETTINGS.bootstrap.read_error_timeout,
        write_error_timeout: SETTINGS.bootstrap.write_error_timeout,
        retry_delay: SETTINGS.bootstrap.retry_delay,
        max_ping: SETTINGS.bootstrap.max_ping,
        max_clock_delta: SETTINGS.bootstrap.max_clock_delta,
        cache_duration: SETTINGS.bootstrap.cache_duration,
        max_simultaneous_bootstraps: SETTINGS.bootstrap.max_simultaneous_bootstraps,
        per_ip_min_interval: SETTINGS.bootstrap.per_ip_min_interval,
        ip_list_max_size: SETTINGS.bootstrap.ip_list_max_size,
        max_bytes_read_write: SETTINGS.bootstrap.max_bytes_read_write,
        max_bootstrap_message_size: MAX_BOOTSTRAP_MESSAGE_SIZE,
        max_datastore_key_length: MAX_DATASTORE_KEY_LENGTH,
        randomness_size_bytes: BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
        thread_count: THREAD_COUNT,
        periods_per_cycle: PERIODS_PER_CYCLE,
        endorsement_count: ENDORSEMENT_COUNT,
        max_advertise_length: MAX_ADVERTISE_LENGTH,
        max_bootstrap_blocks_length: MAX_BOOTSTRAP_BLOCKS,
        max_bootstrap_error_length: MAX_BOOTSTRAP_ERROR_LENGTH,
        max_bootstrap_final_state_parts_size: MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE,
        max_async_pool_changes: MAX_BOOTSTRAP_ASYNC_POOL_CHANGES,
        max_async_pool_length: MAX_ASYNC_POOL_LENGTH,
        max_async_message_data: MAX_ASYNC_MESSAGE_DATA,
        max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
        max_datastore_entry_count: MAX_DATASTORE_ENTRY_COUNT,
        max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
        max_function_name_length: MAX_FUNCTION_NAME_LENGTH,
        max_ledger_changes_count: MAX_LEDGER_CHANGES_COUNT,
        max_parameters_size: MAX_PARAMETERS_SIZE,
        max_op_datastore_entry_count: MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        max_op_datastore_key_length: MAX_OPERATION_DATASTORE_KEY_LENGTH,
        max_op_datastore_value_length: MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        max_changes_slot_count: SETTINGS.ledger.final_history_length as u64,
        max_rolls_length: MAX_ROLLS_COUNT_LENGTH,
        max_production_stats_length: MAX_PRODUCTION_STATS_LENGTH,
        max_credits_length: MAX_DEFERRED_CREDITS_LENGTH,
        max_executed_ops_length: MAX_EXECUTED_OPS_LENGTH,
        max_ops_changes_length: MAX_EXECUTED_OPS_CHANGES_LENGTH,
        consensus_bootstrap_part_size: CONSENSUS_BOOTSTRAP_PART_SIZE,
        max_consensus_block_ids: MAX_CONSENSUS_BLOCKS_IDS,
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
            massa_bootstrap::DefaultEstablisher::default(),
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
        max_send_wait_node_event: SETTINGS.network.max_send_wait_node_event,
        max_send_wait_network_event: SETTINGS.network.max_send_wait_network_event,
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
        max_op_datastore_entry_count: MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        max_op_datastore_key_length: MAX_OPERATION_DATASTORE_KEY_LENGTH,
        max_op_datastore_value_length: MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        max_function_name_length: MAX_FUNCTION_NAME_LENGTH,
        max_parameters_size: MAX_PARAMETERS_SIZE,
        controller_channel_size: NETWORK_CONTROLLER_CHANNEL_SIZE,
        event_channel_size: NETWORK_EVENT_CHANNEL_SIZE,
        node_command_channel_size: NETWORK_NODE_COMMAND_CHANNEL_SIZE,
        node_event_channel_size: NETWORK_NODE_EVENT_CHANNEL_SIZE,
    };

    // launch network controller
    let (network_command_sender, network_event_receiver, network_manager, private_key, node_id) =
        start_network_controller(
            &network_config,
            Establisher::new(),
            bootstrap_state.peers,
            *VERSION,
        )
        .await
        .expect("could not start network controller");

    // give the controller to final state in order for it to feed the cycles
    final_state
        .write()
        .compute_initial_draws()
        .expect("could not compute initial draws"); // TODO: this might just mean a bad bootstrap, no need to panic, just reboot

    // Storage costs constants
    let storage_costs_constants = StorageCostsConstants {
        ledger_cost_per_byte: LEDGER_COST_PER_BYTE,
        ledger_entry_base_cost: LEDGER_COST_PER_BYTE
            .checked_mul_u64(LEDGER_ENTRY_BASE_SIZE as u64)
            .expect("Overflow when creating constant ledger_entry_base_cost"),
        ledger_entry_datastore_base_cost: LEDGER_COST_PER_BYTE
            .checked_mul_u64(LEDGER_ENTRY_DATASTORE_BASE_SIZE as u64)
            .expect("Overflow when creating constant ledger_entry_datastore_base_size"),
    };
    // launch execution module
    let execution_config = ExecutionConfig {
        max_final_events: SETTINGS.execution.max_final_events,
        readonly_queue_length: SETTINGS.execution.readonly_queue_length,
        cursor_delay: SETTINGS.execution.cursor_delay,
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
        stats_time_window_duration: SETTINGS.execution.stats_time_window_duration,
        max_miss_ratio: *POS_MISS_RATE_DEACTIVATION_THRESHOLD,
        max_datastore_key_length: MAX_DATASTORE_KEY_LENGTH,
        max_bytecode_size: MAX_BYTECODE_LENGTH,
        max_datastore_value_size: MAX_DATASTORE_VALUE_LENGTH,
        max_module_cache_size: SETTINGS.execution.max_module_cache_size,
        storage_costs_constants,
        max_read_only_gas: SETTINGS.execution.max_read_only_gas,
        initial_vesting_path: SETTINGS.execution.initial_vesting_path.clone(),
        gas_costs: GasCosts::new(
            SETTINGS.execution.abi_gas_costs_file.clone(),
            SETTINGS.execution.wasm_gas_costs_file.clone(),
        )
        .expect("Failed to load gas costs"),
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
        max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
        max_operation_pool_size_per_thread: SETTINGS.pool.max_pool_size_per_thread,
        max_endorsements_pool_size_per_thread: SETTINGS.pool.max_pool_size_per_thread,
        channels_size: POOL_CONTROLLER_CHANNEL_SIZE,
        broadcast_enabled: SETTINGS.api.enable_ws,
        broadcast_operations_capacity: SETTINGS.pool.broadcast_operations_capacity,
    };

    let pool_channels = PoolChannels {
        operation_sender: broadcast::channel(pool_config.broadcast_operations_capacity).0,
    };

    let (pool_manager, pool_controller) = start_pool_controller(
        pool_config,
        &shared_storage,
        execution_controller.clone(),
        pool_channels.clone(),
    );

    let (protocol_command_sender, protocol_command_receiver) =
        mpsc::channel::<ProtocolCommand>(PROTOCOL_CONTROLLER_CHANNEL_SIZE);

    let consensus_config = ConsensusConfig {
        genesis_timestamp: *GENESIS_TIMESTAMP,
        end_timestamp: *END_TIMESTAMP,
        thread_count: THREAD_COUNT,
        t0: T0,
        genesis_key: GENESIS_KEY.clone(),
        max_discarded_blocks: SETTINGS.consensus.max_discarded_blocks,
        future_block_processing_max_periods: SETTINGS.consensus.future_block_processing_max_periods,
        max_future_processing_blocks: SETTINGS.consensus.max_future_processing_blocks,
        max_dependency_blocks: SETTINGS.consensus.max_dependency_blocks,
        delta_f0: DELTA_F0,
        operation_validity_periods: OPERATION_VALIDITY_PERIODS,
        periods_per_cycle: PERIODS_PER_CYCLE,
        stats_timespan: SETTINGS.consensus.stats_timespan,
        max_send_wait: SETTINGS.consensus.max_send_wait,
        force_keep_final_periods: SETTINGS.consensus.force_keep_final_periods,
        endorsement_count: ENDORSEMENT_COUNT,
        block_db_prune_interval: SETTINGS.consensus.block_db_prune_interval,
        max_item_return_count: SETTINGS.consensus.max_item_return_count,
        max_gas_per_block: MAX_GAS_PER_BLOCK,
        channel_size: CHANNEL_SIZE,
        bootstrap_part_size: CONSENSUS_BOOTSTRAP_PART_SIZE,
        broadcast_enabled: SETTINGS.api.enable_ws,
        broadcast_blocks_headers_capacity: SETTINGS.consensus.broadcast_blocks_headers_capacity,
        broadcast_blocks_capacity: SETTINGS.consensus.broadcast_blocks_capacity,
        broadcast_filled_blocks_capacity: SETTINGS.consensus.broadcast_filled_blocks_capacity,
    };

    let (consensus_event_sender, consensus_event_receiver) =
        crossbeam_channel::bounded(CHANNEL_SIZE);
    let consensus_channels = ConsensusChannels {
        execution_controller: execution_controller.clone(),
        selector_controller: selector_controller.clone(),
        pool_command_sender: pool_controller.clone(),
        controller_event_tx: consensus_event_sender,
        protocol_command_sender: ProtocolCommandSender(protocol_command_sender.clone()),
        block_header_sender: broadcast::channel(consensus_config.broadcast_blocks_headers_capacity)
            .0,
        block_sender: broadcast::channel(consensus_config.broadcast_blocks_capacity).0,
        filled_block_sender: broadcast::channel(consensus_config.broadcast_filled_blocks_capacity)
            .0,
    };

    let (consensus_controller, consensus_manager) = start_consensus_worker(
        consensus_config,
        consensus_channels.clone(),
        bootstrap_state.graph,
        shared_storage.clone(),
    );

    // launch protocol controller
    let protocol_config = ProtocolConfig {
        thread_count: THREAD_COUNT,
        ask_block_timeout: SETTINGS.protocol.ask_block_timeout,
        max_known_blocks_size: SETTINGS.protocol.max_known_blocks_size,
        max_node_known_blocks_size: SETTINGS.protocol.max_node_known_blocks_size,
        max_node_wanted_blocks_size: SETTINGS.protocol.max_node_wanted_blocks_size,
        max_known_ops_size: SETTINGS.protocol.max_known_ops_size,
        max_node_known_ops_size: SETTINGS.protocol.max_node_known_ops_size,
        max_known_endorsements_size: SETTINGS.protocol.max_known_endorsements_size,
        max_node_known_endorsements_size: SETTINGS.protocol.max_node_known_endorsements_size,
        max_simultaneous_ask_blocks_per_node: SETTINGS
            .protocol
            .max_simultaneous_ask_blocks_per_node,
        max_send_wait: SETTINGS.protocol.max_send_wait,
        operation_batch_buffer_capacity: SETTINGS.protocol.operation_batch_buffer_capacity,
        operation_announcement_buffer_capacity: SETTINGS
            .protocol
            .operation_announcement_buffer_capacity,
        operation_batch_proc_period: SETTINGS.protocol.operation_batch_proc_period,
        asked_operations_pruning_period: SETTINGS.protocol.asked_operations_pruning_period,
        operation_announcement_interval: SETTINGS.protocol.operation_announcement_interval,
        max_operations_per_message: SETTINGS.protocol.max_operations_per_message,
        max_serialized_operations_size_per_block: MAX_BLOCK_SIZE as usize,
        controller_channel_size: PROTOCOL_CONTROLLER_CHANNEL_SIZE,
        event_channel_size: PROTOCOL_EVENT_CHANNEL_SIZE,
        genesis_timestamp: *GENESIS_TIMESTAMP,
        t0: T0,
        max_operations_propagation_time: SETTINGS.protocol.max_operations_propagation_time,
        max_endorsements_propagation_time: SETTINGS.protocol.max_endorsements_propagation_time,
    };

    let protocol_senders = ProtocolSenders {
        network_command_sender: network_command_sender.clone(),
    };

    let protocol_receivers = ProtocolReceivers {
        network_event_receiver,
        protocol_command_receiver,
    };

    let protocol_manager = start_protocol_controller(
        protocol_config,
        protocol_receivers,
        protocol_senders.clone(),
        consensus_controller.clone(),
        pool_controller.clone(),
        shared_storage.clone(),
    )
    .await
    .expect("could not start protocol controller");

    // launch factory
    let factory_config = FactoryConfig {
        thread_count: THREAD_COUNT,
        genesis_timestamp: *GENESIS_TIMESTAMP,
        t0: T0,
        initial_delay: SETTINGS.factory.initial_delay,
        max_block_size: MAX_BLOCK_SIZE as u64,
        max_block_gas: MAX_GAS_PER_BLOCK,
    };
    let factory_channels = FactoryChannels {
        selector: selector_controller.clone(),
        consensus: consensus_controller.clone(),
        pool: pool_controller.clone(),
        protocol: ProtocolCommandSender(protocol_command_sender.clone()),
        storage: shared_storage.clone(),
    };
    let factory_manager = start_factory(factory_config, node_wallet.clone(), factory_channels);

    // launch bootstrap server
    let addr = &bootstrap_config.listen_addr.unwrap();
    let bootstrap_manager = start_bootstrap_server::<TcpStream>(
        consensus_controller.clone(),
        network_command_sender.clone(),
        final_state.clone(),
        bootstrap_config,
        massa_bootstrap::DefaultEstablisher::new()
            .get_listener(addr)
            .unwrap(),
        private_key,
        *VERSION,
    )
    .unwrap();

    let api_config: APIConfig = APIConfig {
        bind_private: SETTINGS.api.bind_private,
        bind_public: SETTINGS.api.bind_public,
        bind_api: SETTINGS.api.bind_api,
        draw_lookahead_period_count: SETTINGS.api.draw_lookahead_period_count,
        max_arguments: SETTINGS.api.max_arguments,
        openrpc_spec_path: SETTINGS.api.openrpc_spec_path.clone(),
        bootstrap_whitelist_path: SETTINGS.bootstrap.bootstrap_whitelist_path.clone(),
        bootstrap_blacklist_path: SETTINGS.bootstrap.bootstrap_blacklist_path.clone(),
        max_request_body_size: SETTINGS.api.max_request_body_size,
        max_response_body_size: SETTINGS.api.max_response_body_size,
        max_connections: SETTINGS.api.max_connections,
        max_subscriptions_per_connection: SETTINGS.api.max_subscriptions_per_connection,
        max_log_length: SETTINGS.api.max_log_length,
        allow_hosts: SETTINGS.api.allow_hosts.clone(),
        batch_requests_supported: SETTINGS.api.batch_requests_supported,
        ping_interval: SETTINGS.api.ping_interval,
        enable_http: SETTINGS.api.enable_http,
        enable_ws: SETTINGS.api.enable_ws,
        max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
        max_op_datastore_entry_count: MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        max_op_datastore_key_length: MAX_OPERATION_DATASTORE_KEY_LENGTH,
        max_op_datastore_value_length: MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        max_function_name_length: MAX_FUNCTION_NAME_LENGTH,
        max_parameter_size: MAX_PARAMETERS_SIZE,
        thread_count: THREAD_COUNT,
        genesis_timestamp: *GENESIS_TIMESTAMP,
        t0: T0,
        periods_per_cycle: PERIODS_PER_CYCLE,
    };

    // spawn Massa API
    let api = API::<ApiV2>::new(
        consensus_controller.clone(),
        consensus_channels,
        execution_controller.clone(),
        pool_channels,
        api_config.clone(),
        *VERSION,
    );
    let api_handle = api
        .serve(&SETTINGS.api.bind_api, &api_config)
        .await
        .expect("failed to start MASSA API");

    // Disable WebSockets for Private and Public API's
    let mut api_config = api_config.clone();
    api_config.enable_ws = false;

    // spawn private API
    let (api_private, api_private_stop_rx) = API::<Private>::new(
        network_command_sender.clone(),
        execution_controller.clone(),
        api_config.clone(),
        node_wallet,
    );
    let api_private_handle = api_private
        .serve(&SETTINGS.api.bind_private, &api_config)
        .await
        .expect("failed to start PRIVATE API");

    // spawn public API
    let api_public = API::<Public>::new(
        consensus_controller.clone(),
        execution_controller.clone(),
        api_config.clone(),
        selector_controller.clone(),
        pool_controller.clone(),
        ProtocolCommandSender(protocol_command_sender.clone()),
        network_config,
        *VERSION,
        network_command_sender.clone(),
        node_id,
        shared_storage.clone(),
    );
    let api_public_handle = api_public
        .serve(&SETTINGS.api.bind_public, &api_config)
        .await
        .expect("failed to start PUBLIC API");

    #[cfg(feature = "deadlock_detection")]
    {
        // only for #[cfg]
        use parking_lot::deadlock;
        use std::thread;

        let interval = Duration::from_secs(_args.dl_interval);
        warn!("deadlocks detector will run every {:?}", interval);

        // Create a background thread which checks for deadlocks at the defined interval
        let thread_builder = thread::Builder::new().name("deadlock-detection".into());
        thread_builder
            .spawn(move || loop {
                thread::sleep(interval);
                let deadlocks = deadlock::check_deadlock();
                if deadlocks.is_empty() {
                    continue;
                }
                warn!("{} deadlocks detected", deadlocks.len());
                for (i, threads) in deadlocks.iter().enumerate() {
                    warn!("Deadlock #{}", i);
                    for t in threads {
                        warn!("Thread Id {:#?}", t.thread_id());
                        warn!("{:#?}", t.backtrace());
                    }
                }
            })
            .expect("failed to spawn thread : deadlock-detection");
    }
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
        api_handle,
    )
}

struct Managers {
    bootstrap_manager: Option<BootstrapManager<TcpStream>>,
    consensus_manager: Box<dyn ConsensusManager>,
    execution_manager: Box<dyn ExecutionManager>,
    selector_manager: Box<dyn SelectorManager>,
    pool_manager: Box<dyn PoolManager>,
    protocol_manager: ProtocolManager,
    network_manager: NetworkManager,
    factory_manager: Box<dyn FactoryManager>,
}

async fn stop(
    _consensus_event_receiver: Receiver<ConsensusEvent>,
    Managers {
        bootstrap_manager,
        mut execution_manager,
        mut consensus_manager,
        mut selector_manager,
        mut pool_manager,
        protocol_manager,
        network_manager,
        mut factory_manager,
    }: Managers,
    api_private_handle: StopHandle,
    api_public_handle: StopHandle,
    api_handle: StopHandle,
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

    // stop Massa API
    api_handle.stop();

    // stop factory
    factory_manager.stop();

    // stop protocol controller
    let network_event_receiver = protocol_manager
        .stop()
        .await
        .expect("protocol shutdown failed");

    // stop consensus
    consensus_manager.stop();

    // stop pool
    pool_manager.stop();

    // stop execution controller
    execution_manager.stop();

    // stop selector controller
    selector_manager.stop();

    // stop pool controller
    // TODO
    //let protocol_pool_event_receiver = pool_manager.stop().await.expect("pool shutdown failed");

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

    #[cfg(feature = "deadlock_detection")]
    /// Deadlocks detector
    #[structopt(
        name = "deadlocks interval",
        about = "Define the interval of launching a deadlocks checking.",
        short = "i",
        long = "dli",
        default_value = "10"
    )]
    dl_interval: u64,
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

#[paw::main]
fn main(args: Args) -> anyhow::Result<()> {
    let tokio_rt = tokio::runtime::Builder::new_multi_thread()
        .thread_name_fn(|| {
            static ATOMIC_ID: AtomicUsize = AtomicUsize::new(0);
            let id = ATOMIC_ID.fetch_add(1, Ordering::SeqCst);
            format!("tokio-node-{}", id)
        })
        .enable_all()
        .build()
        .unwrap();

    tokio_rt.block_on(run(args))
}

async fn run(args: Args) -> anyhow::Result<()> {
    use tracing_subscriber::prelude::*;
    // spawn the console server in the background, returning a `Layer`:
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
    let node_wallet = load_wallet(args.password.clone(), &SETTINGS.factory.staking_wallet_path)?;

    loop {
        let (
            consensus_event_receiver,
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
            api_handle,
        ) = launch(&args, node_wallet.clone()).await;

        // interrupt signal listener
        let (tx, rx) = crossbeam_channel::bounded(1);
        let interrupt_signal_listener = tokio::spawn(async move {
            signal::ctrl_c().await.unwrap();
            tx.send(()).unwrap();
        });

        // loop over messages
        let restart = loop {
            massa_trace!("massa-node.main.run.select", {});
            match consensus_event_receiver.try_recv() {
                Ok(evt) => match evt {
                    ConsensusEvent::NeedSync => {
                        warn!("in response to a desynchronization, the node is going to bootstrap again");
                        break true;
                    }
                    ConsensusEvent::Stop => {
                        break false;
                    }
                },
                Err(TryRecvError::Disconnected) => {
                    error!("consensus_event_receiver.wait_event disconnected");
                    break false;
                }
                _ => {}
            };

            match api_private_stop_rx.try_recv() {
                Ok(_) => {
                    info!("stop command received from private API");
                    break false;
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    error!("api_private_stop_rx disconnected");
                    break false;
                }
                _ => {}
            }
            match rx.try_recv() {
                Ok(_) => {
                    info!("interrupt signal received");
                    break false;
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    error!("interrupt_signal_listener disconnected");
                    break false;
                }
                _ => {}
            }
            sleep(Duration::from_millis(100));
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
            api_handle,
        )
        .await;

        if !restart {
            break;
        }
        interrupt_signal_listener.abort();
    }
    Ok(())
}
