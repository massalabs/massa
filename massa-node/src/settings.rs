// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Build here the default node settings from the configuration file toml
use std::path::PathBuf;

use massa_bootstrap::IpType;
use massa_models::{config::build_massa_settings, node::NodeId};
use massa_time::MassaTime;
use serde::Deserialize;
use std::net::{IpAddr, SocketAddr};

lazy_static::lazy_static! {
    pub(crate)  static ref SETTINGS: Settings = build_massa_settings("massa-node", "MASSA_NODE");
}

#[derive(Debug, Deserialize, Clone)]
pub(crate)  struct LoggingSettings {
    pub(crate)  level: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate)  struct ExecutionSettings {
    pub(crate)  max_final_events: usize,
    pub(crate)  readonly_queue_length: usize,
    pub(crate)  cursor_delay: MassaTime,
    pub(crate)  stats_time_window_duration: MassaTime,
    pub(crate)  max_read_only_gas: u64,
    pub(crate)  abi_gas_costs_file: PathBuf,
    pub(crate)  wasm_gas_costs_file: PathBuf,
    pub(crate)  initial_vesting_path: PathBuf,
    pub(crate)  hd_cache_path: PathBuf,
    pub(crate)  lru_cache_size: u32,
    pub(crate)  hd_cache_size: usize,
    pub(crate)  snip_amount: usize,
    /// slot execution outputs channel capacity
    pub(crate)  broadcast_slot_execution_output_channel_capacity: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate)  struct SelectionSettings {
    pub(crate)  initial_rolls_path: PathBuf,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate)  struct LedgerSettings {
    pub(crate)  initial_ledger_path: PathBuf,
    pub(crate)  disk_ledger_path: PathBuf,
    pub(crate)  final_history_length: usize,
}

/// Bootstrap configuration.
#[derive(Debug, Deserialize, Clone)]
pub(crate)  struct BootstrapSettings {
    pub(crate)  bootstrap_list: Vec<(SocketAddr, NodeId)>,
    pub(crate)  bootstrap_protocol: IpType,
    pub(crate)  bootstrap_whitelist_path: PathBuf,
    pub(crate)  bootstrap_blacklist_path: PathBuf,
    pub(crate)  bind: Option<SocketAddr>,
    pub(crate)  connect_timeout: MassaTime,
    pub(crate)  read_timeout: MassaTime,
    pub(crate)  write_timeout: MassaTime,
    pub(crate)  read_error_timeout: MassaTime,
    pub(crate)  write_error_timeout: MassaTime,
    pub(crate)  retry_delay: MassaTime,
    pub(crate)  max_ping: MassaTime,
    pub(crate)  max_clock_delta: MassaTime,
    pub(crate)  cache_duration: MassaTime,
    pub(crate)  max_simultaneous_bootstraps: u32,
    pub(crate)  per_ip_min_interval: MassaTime,
    pub(crate)  ip_list_max_size: usize,
    pub(crate)  max_bytes_read_write: f64,
    /// Allocated time with which to manage the bootstrap process
    pub(crate)  bootstrap_timeout: MassaTime,
}

/// Factory settings
#[derive(Debug, Deserialize, Clone)]
pub(crate)  struct FactorySettings {
    /// Initial delay
    pub(crate)  initial_delay: MassaTime,
    /// Staking wallet file
    pub(crate)  staking_wallet_path: PathBuf,
}

/// Pool configuration, read from a file configuration
#[derive(Debug, Deserialize, Clone)]
pub(crate)  struct PoolSettings {
    pub(crate)  max_pool_size_per_thread: usize,
    pub(crate)  max_operation_future_validity_start_periods: u64,
    pub(crate)  max_endorsement_count: u64,
    pub(crate)  max_item_return_count: usize,
    /// endorsements channel capacity
    pub(crate)  broadcast_endorsements_channel_capacity: usize,
    /// operations channel capacity
    pub(crate)  broadcast_operations_channel_capacity: usize,
}

/// API and server configuration, read from a file configuration.
#[derive(Debug, Deserialize, Clone)]
pub(crate)  struct APISettings {
    pub(crate)  draw_lookahead_period_count: u64,
    pub(crate)  bind_private: SocketAddr,
    pub(crate)  bind_public: SocketAddr,
    pub(crate)  bind_api: SocketAddr,
    pub(crate)  max_arguments: u64,
    pub(crate)  openrpc_spec_path: PathBuf,
    pub(crate)  max_request_body_size: u32,
    pub(crate)  max_response_body_size: u32,
    pub(crate)  max_connections: u32,
    pub(crate)  max_subscriptions_per_connection: u32,
    pub(crate)  max_log_length: u32,
    pub(crate)  allow_hosts: Vec<String>,
    pub(crate)  batch_requests_supported: bool,
    pub(crate)  ping_interval: MassaTime,
    pub(crate)  enable_http: bool,
    pub(crate)  enable_ws: bool,
    // whether to broadcast for blocks, endorsement and operations
    pub(crate)  enable_broadcast: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate)  struct Settings {
    pub(crate)  logging: LoggingSettings,
    pub(crate)  protocol: ProtocolSettings,
    pub(crate)  consensus: ConsensusSettings,
    pub(crate)  api: APISettings,
    pub(crate)  bootstrap: BootstrapSettings,
    pub(crate)  pool: PoolSettings,
    pub(crate)  execution: ExecutionSettings,
    pub(crate)  ledger: LedgerSettings,
    pub(crate)  selector: SelectionSettings,
    pub(crate)  factory: FactorySettings,
    pub(crate)  grpc: GrpcSettings,
}

/// Consensus configuration
/// Assumes `thread_count >= 1, t0_millis >= 1, t0_millis % thread_count == 0`
#[derive(Debug, Deserialize, Clone)]
pub(crate)  struct ConsensusSettings {
    /// Maximum number of blocks allowed in discarded blocks.
    pub(crate)  max_discarded_blocks: usize,
    /// If a block is `future_block_processing_max_periods` periods in the future, it is just discarded.
    pub(crate)  future_block_processing_max_periods: u64,
    /// Maximum number of blocks allowed in `FutureIncomingBlocks`.
    pub(crate)  max_future_processing_blocks: usize,
    /// Maximum number of blocks allowed in `DependencyWaitingBlocks`.
    pub(crate)  max_dependency_blocks: usize,
    /// stats time span
    pub(crate)  stats_timespan: MassaTime,
    /// max event send wait
    pub(crate)  max_send_wait: MassaTime,
    /// force keep at least this number of final periods in RAM for each thread
    pub(crate)  force_keep_final_periods: u64,
    /// old blocks are pruned every `block_db_prune_interval`
    pub(crate)  block_db_prune_interval: MassaTime,
    /// max number of items returned while querying
    pub(crate)  max_item_return_count: usize,
    /// blocks headers channel capacity
    pub(crate)  broadcast_blocks_headers_channel_capacity: usize,
    /// blocks channel capacity
    pub(crate)  broadcast_blocks_channel_capacity: usize,
    /// filled blocks channel capacity
    pub(crate)  broadcast_filled_blocks_channel_capacity: usize,
}

/// Protocol Configuration, read from toml user configuration file
#[derive(Debug, Deserialize, Clone)]
pub(crate)  struct ProtocolSettings {
    /// after `ask_block_timeout` milliseconds we try to ask a block to another node
    pub(crate)  ask_block_timeout: MassaTime,
    /// max known blocks of current nodes we keep in memory (by node)
    pub(crate)  max_known_blocks_size: usize,
    /// max known blocks of foreign nodes we keep in memory (by node)
    pub(crate)  max_node_known_blocks_size: usize,
    /// max wanted blocks per node kept in memory
    pub(crate)  max_node_wanted_blocks_size: usize,
    /// max known operations current node kept in memory
    pub(crate)  max_known_ops_size: usize,
    /// max known operations of foreign nodes we keep in memory (by node)
    pub(crate)  max_node_known_ops_size: usize,
    /// max known endorsements by our node that we kept in memory
    pub(crate)  max_known_endorsements_size: usize,
    /// max known endorsements of foreign nodes we keep in memory (by node)
    pub(crate)  max_node_known_endorsements_size: usize,
    /// we ask for the same block `max_simultaneous_ask_blocks_per_node` times at the same time
    pub(crate)  max_simultaneous_ask_blocks_per_node: usize,
    /// Max wait time for sending a Network or Node event.
    pub(crate)  max_send_wait: MassaTime,
    /// Maximum number of batches in the memory buffer.
    /// Dismiss the new batches if overflow
    pub(crate)  operation_batch_buffer_capacity: usize,
    /// Maximum number of operations in the announcement buffer.
    /// Immediately announce if overflow.
    pub(crate)  operation_announcement_buffer_capacity: usize,
    /// Start processing batches in the buffer each `operation_batch_proc_period` in millisecond
    pub(crate)  operation_batch_proc_period: MassaTime,
    /// All operations asked are prune each `operation_asked_pruning_period` millisecond
    pub(crate)  asked_operations_pruning_period: MassaTime,
    /// Interval at which operations are announced in batches.
    pub(crate)  operation_announcement_interval: MassaTime,
    /// Maximum of operations sent in one message.
    pub(crate)  max_operations_per_message: u64,
    /// Time threshold after which operation are not propagated
    pub(crate)  max_operations_propagation_time: MassaTime,
    /// Time threshold after which operation are not propagated
    pub(crate)  max_endorsements_propagation_time: MassaTime,
    /// Path for initial peers
    pub(crate)  initial_peers_file: PathBuf,
    /// Keypair
    pub(crate)  keypair_file: PathBuf,
    /// Ip we are bind to listen to
    pub(crate)  bind: SocketAddr,
    /// Ip seen by others. If none the bind ip is used
    pub(crate)  routable_ip: Option<IpAddr>,
    /// Time threshold to have a connection to a node
    pub(crate)  connect_timeout: MassaTime,
    /// Max number of connection in
    pub(crate)  max_incoming_connections: usize,
    /// Max number of connection out
    pub(crate)  max_outgoing_connections: usize,
    /// Number of tester threads
    pub(crate)  thread_tester_count: u8,
    /// Number of bytes we can read/write by seconds in a connection (must be a 10 multiple)
    pub(crate)  read_write_limit_bytes_per_second: u64,
}

/// gRPC settings
/// the gRPC settings
#[derive(Debug, Deserialize, Clone)]
pub(crate)  struct GrpcSettings {
    /// whether to enable gRPC
    pub(crate)  enabled: bool,
    /// whether to accept HTTP/1.1 requests
    pub(crate)  accept_http1: bool,
    /// whether to enable CORS. Works only if `accept_http1` is true
    pub(crate)  enable_cors: bool,
    /// whether to enable gRPC health service
    pub(crate)  enable_health: bool,
    /// whether to enable gRPC reflection
    pub(crate)  enable_reflection: bool,
    /// bind for the Massa gRPC API
    pub(crate)  bind: SocketAddr,
    /// which compression encodings does the server accept for requests
    pub(crate)  accept_compressed: Option<String>,
    /// which compression encodings might the server use for responses
    pub(crate)  send_compressed: Option<String>,
    /// limits the maximum size of a decoded message. Defaults to 4MB
    pub(crate)  max_decoding_message_size: usize,
    /// limits the maximum size of an encoded message. Defaults to 4MB
    pub(crate)  max_encoding_message_size: usize,
    /// limits the maximum size of streaming channel
    pub(crate)  max_channel_size: usize,
    /// set the concurrency limit applied to on requests inbound per connection. Defaults to 32
    pub(crate)  concurrency_limit_per_connection: usize,
    /// set a timeout on for all request handlers
    pub(crate)  timeout: MassaTime,
    /// sets the SETTINGS_INITIAL_WINDOW_SIZE spec option for HTTP2 stream-level flow control. Default is 65,535
    pub(crate)  initial_stream_window_size: Option<u32>,
    /// sets the max connection-level flow control for HTTP2. Default is 65,535
    pub(crate)  initial_connection_window_size: Option<u32>,
    /// sets the SETTINGS_MAX_CONCURRENT_STREAMS spec option for HTTP2 connections. Default is no limit (`None`)
    pub(crate)  max_concurrent_streams: Option<u32>,
    /// set whether TCP keepalive messages are enabled on accepted connections
    pub(crate)  tcp_keepalive: Option<MassaTime>,
    /// set the value of `TCP_NODELAY` option for accepted connections. Enabled by default
    pub(crate)  tcp_nodelay: bool,
    /// set whether HTTP2 Ping frames are enabled on accepted connections. Default is no HTTP2 keepalive (`None`)
    pub(crate)  http2_keepalive_interval: Option<MassaTime>,
    /// sets a timeout for receiving an acknowledgement of the keepalive ping. Default is 20 seconds
    pub(crate)  http2_keepalive_timeout: Option<MassaTime>,
    /// sets whether to use an adaptive flow control. Defaults to false
    pub(crate)  http2_adaptive_window: Option<bool>,
    /// sets the maximum frame size to use for HTTP2(must be within 16,384 and 16,777,215). If not set, will default from underlying transport
    pub(crate)  max_frame_size: Option<u32>,
    /// when looking for next draw we want to look at max `draw_lookahead_period_count`
    pub(crate)  draw_lookahead_period_count: u64,
    /// max number of block ids that can be included in a single request
    pub(crate)  max_block_ids_per_request: u32,
    /// max number of operation ids that can be included in a single request
    pub(crate)  max_operation_ids_per_request: u32,
}

#[cfg(test)]
#[test]
fn test_load_node_config() {
    let _ = *SETTINGS;
}
