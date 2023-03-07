// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Build here the default node settings from the configuration file toml
use std::path::PathBuf;

use enum_map::EnumMap;
use massa_bootstrap::IpType;
use massa_models::{config::build_massa_settings, node::NodeId};
use massa_time::MassaTime;
use serde::Deserialize;
use std::net::{IpAddr, SocketAddr};

use massa_network_exports::{settings::PeerTypeConnectionConfig, PeerType};

lazy_static::lazy_static! {
    pub static ref SETTINGS: Settings = build_massa_settings("massa-node", "MASSA_NODE");
}

#[derive(Debug, Deserialize, Clone)]
pub struct LoggingSettings {
    pub level: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SnapshotSettings {
    pub final_state_path: PathBuf,
    pub last_start_period: u64
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExecutionSettings {
    pub max_final_events: usize,
    pub readonly_queue_length: usize,
    pub cursor_delay: MassaTime,
    pub stats_time_window_duration: MassaTime,
    pub max_read_only_gas: u64,
    pub abi_gas_costs_file: PathBuf,
    pub wasm_gas_costs_file: PathBuf,
    pub max_module_cache_size: u32,
    pub initial_vesting_path: PathBuf,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SelectionSettings {
    pub max_draw_cache: usize,
    pub initial_rolls_path: PathBuf,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LedgerSettings {
    pub initial_ledger_path: PathBuf,
    pub disk_ledger_path: PathBuf,
    pub final_history_length: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NetworkSettings {
    pub bind: SocketAddr,
    pub routable_ip: Option<IpAddr>,
    pub protocol_port: u16,
    pub connect_timeout: MassaTime,
    pub wakeup_interval: MassaTime,
    pub initial_peers_file: PathBuf,
    pub peers_file: PathBuf,
    pub keypair_file: PathBuf,
    pub peer_types_config: EnumMap<PeerType, PeerTypeConnectionConfig>,
    pub max_in_connections_per_ip: usize,
    pub max_idle_peers: usize,
    pub max_banned_peers: usize,
    pub peers_file_dump_interval: MassaTime,
    pub message_timeout: MassaTime,
    pub ask_peer_list_interval: MassaTime,
    pub max_send_wait_node_event: MassaTime,
    pub max_send_wait_network_event: MassaTime,
    pub ban_timeout: MassaTime,
    pub peer_list_send_timeout: MassaTime,
    pub max_in_connection_overflow: usize,
    pub max_operations_per_message: u32,
    pub max_bytes_read: f64,
    pub max_bytes_write: f64,
}

/// Bootstrap configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct BootstrapSettings {
    pub bootstrap_list: Vec<(SocketAddr, NodeId)>,
    pub bootstrap_protocol: IpType,
    pub bootstrap_whitelist_path: PathBuf,
    pub bootstrap_blacklist_path: PathBuf,
    pub bind: Option<SocketAddr>,
    pub connect_timeout: MassaTime,
    pub read_timeout: MassaTime,
    pub write_timeout: MassaTime,
    pub read_error_timeout: MassaTime,
    pub write_error_timeout: MassaTime,
    pub retry_delay: MassaTime,
    pub max_ping: MassaTime,
    pub max_clock_delta: MassaTime,
    pub cache_duration: MassaTime,
    pub max_simultaneous_bootstraps: u32,
    pub per_ip_min_interval: MassaTime,
    pub ip_list_max_size: usize,
    pub max_bytes_read_write: f64,
    /// Allocated time with which to manage the bootstrap process
    pub bootstrap_timeout: MassaTime,
}

/// Factory settings
#[derive(Debug, Deserialize, Clone)]
pub struct FactorySettings {
    /// Initial delay
    pub initial_delay: MassaTime,
    /// Staking wallet file
    pub staking_wallet_path: PathBuf,
}

/// Pool configuration, read from a file configuration
#[derive(Debug, Deserialize, Clone)]
pub struct PoolSettings {
    pub max_pool_size_per_thread: usize,
    pub max_operation_future_validity_start_periods: u64,
    pub max_endorsement_count: u64,
    pub max_item_return_count: usize,
    /// operations sender(channel) capacity
    pub broadcast_operations_capacity: usize,
}

/// API and server configuration, read from a file configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct APISettings {
    pub draw_lookahead_period_count: u64,
    pub bind_private: SocketAddr,
    pub bind_public: SocketAddr,
    pub bind_api: SocketAddr,
    pub max_arguments: u64,
    pub openrpc_spec_path: PathBuf,
    pub max_request_body_size: u32,
    pub max_response_body_size: u32,
    pub max_connections: u32,
    pub max_subscriptions_per_connection: u32,
    pub max_log_length: u32,
    pub allow_hosts: Vec<String>,
    pub batch_requests_supported: bool,
    pub ping_interval: MassaTime,
    pub enable_http: bool,
    pub enable_ws: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub logging: LoggingSettings,
    pub protocol: ProtocolSettings,
    pub network: NetworkSettings,
    pub consensus: ConsensusSettings,
    pub api: APISettings,
    pub bootstrap: BootstrapSettings,
    pub pool: PoolSettings,
    pub execution: ExecutionSettings,
    pub ledger: LedgerSettings,
    pub selector: SelectionSettings,
    pub factory: FactorySettings,
    pub snapshot: SnapshotSettings,
}

/// Consensus configuration
/// Assumes `thread_count >= 1, t0_millis >= 1, t0_millis % thread_count == 0`
#[derive(Debug, Deserialize, Clone)]
pub struct ConsensusSettings {
    /// Maximum number of blocks allowed in discarded blocks.
    pub max_discarded_blocks: usize,
    /// If a block is `future_block_processing_max_periods` periods in the future, it is just discarded.
    pub future_block_processing_max_periods: u64,
    /// Maximum number of blocks allowed in `FutureIncomingBlocks`.
    pub max_future_processing_blocks: usize,
    /// Maximum number of blocks allowed in `DependencyWaitingBlocks`.
    pub max_dependency_blocks: usize,
    /// stats time span
    pub stats_timespan: MassaTime,
    /// max event send wait
    pub max_send_wait: MassaTime,
    /// force keep at least this number of final periods in RAM for each thread
    pub force_keep_final_periods: u64,
    /// old blocks are pruned every `block_db_prune_interval`
    pub block_db_prune_interval: MassaTime,
    /// max number of items returned while querying
    pub max_item_return_count: usize,
    /// blocks headers sender(channel) capacity
    pub broadcast_blocks_headers_capacity: usize,
    /// blocks sender(channel) capacity
    pub broadcast_blocks_capacity: usize,
    /// filled blocks sender(channel) capacity
    pub broadcast_filled_blocks_capacity: usize,
}

/// Protocol Configuration, read from toml user configuration file
#[derive(Debug, Deserialize, Clone, Copy)]
pub struct ProtocolSettings {
    /// after `ask_block_timeout` milliseconds we try to ask a block to another node
    pub ask_block_timeout: MassaTime,
    /// max known blocks of current nodes we keep in memory (by node)
    pub max_known_blocks_size: usize,
    /// max known blocks of foreign nodes we keep in memory (by node)
    pub max_node_known_blocks_size: usize,
    /// max wanted blocks per node kept in memory
    pub max_node_wanted_blocks_size: usize,
    /// max known operations current node kept in memory
    pub max_known_ops_size: usize,
    /// max known operations of foreign nodes we keep in memory (by node)
    pub max_node_known_ops_size: usize,
    /// max known endorsements by our node that we kept in memory
    pub max_known_endorsements_size: usize,
    /// max known endorsements of foreign nodes we keep in memory (by node)
    pub max_node_known_endorsements_size: usize,
    /// we ask for the same block `max_simultaneous_ask_blocks_per_node` times at the same time
    pub max_simultaneous_ask_blocks_per_node: usize,
    /// Max wait time for sending a Network or Node event.
    pub max_send_wait: MassaTime,
    /// Maximum number of batches in the memory buffer.
    /// Dismiss the new batches if overflow
    pub operation_batch_buffer_capacity: usize,
    /// Maximum number of operations in the announcement buffer.
    /// Immediately announce if overflow.
    pub operation_announcement_buffer_capacity: usize,
    /// Start processing batches in the buffer each `operation_batch_proc_period` in millisecond
    pub operation_batch_proc_period: MassaTime,
    /// All operations asked are prune each `operation_asked_pruning_period` millisecond
    pub asked_operations_pruning_period: MassaTime,
    /// Interval at which operations are announced in batches.
    pub operation_announcement_interval: MassaTime,
    /// Maximum of operations sent in one message.
    pub max_operations_per_message: u64,
    /// Time threshold after which operation are not propagated
    pub max_operations_propagation_time: MassaTime,
    /// Time threshold after which operation are not propagated
    pub max_endorsements_propagation_time: MassaTime,
}

#[cfg(test)]
#[test]
fn test_load_node_config() {
    let _ = *SETTINGS;
}
