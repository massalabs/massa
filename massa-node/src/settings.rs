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
pub struct ExecutionSettings {
    pub max_final_events: usize,
    pub readonly_queue_length: usize,
    pub cursor_delay: MassaTime,
    pub stats_time_window_duration: MassaTime,
    pub max_read_only_gas: u64,
    pub abi_gas_costs_file: PathBuf,
    pub wasm_gas_costs_file: PathBuf,
    pub initial_vesting_path: PathBuf,
    pub hd_cache_path: PathBuf,
    pub lru_cache_size: u32,
    pub hd_cache_size: usize,
    pub snip_amount: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SelectionSettings {
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
    /// endorsements channel capacity
    pub broadcast_endorsements_channel_capacity: usize,
    /// operations channel capacity
    pub broadcast_operations_channel_capacity: usize,
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
    // whether to broadcast for blocks, endorsement and operations
    pub enable_broadcast: bool,
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
    pub grpc: GrpcSettings,
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
    /// blocks headers channel capacity
    pub broadcast_blocks_headers_channel_capacity: usize,
    /// blocks channel capacity
    pub broadcast_blocks_channel_capacity: usize,
    /// filled blocks channel capacity
    pub broadcast_filled_blocks_channel_capacity: usize,
}

/// Protocol Configuration, read from toml user configuration file
#[derive(Debug, Deserialize, Clone)]
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
    /// Path for initial peers
    pub initial_peers_file: PathBuf,
    /// Keypair
    pub keypair_file: PathBuf,
    /// Ip we are bind to listen to
    pub bind: SocketAddr,
    /// Ip seen by others. If none the bind ip is used
    pub routable_ip: Option<IpAddr>,
    /// Port we are bind to connect to
    pub protocol_port: u16,
    /// Time threshold to have a connection to a node
    pub connect_timeout: MassaTime,
    /// Max number of connection in
    pub max_incoming_connections: usize,
    /// Max number of connection out
    pub max_outgoing_connections: usize,
    /// Number of tester threads
    pub thread_tester_count: u8,
}

/// gRPC settings
/// the gRPC settings
#[derive(Debug, Deserialize, Clone)]
pub struct GrpcSettings {
    /// whether to enable gRPC
    pub enabled: bool,
    /// whether to accept HTTP/1.1 requests
    pub accept_http1: bool,
    /// whether to enable CORS. Works only if `accept_http1` is true
    pub enable_cors: bool,
    /// whether to enable gRPC reflection
    pub enable_reflection: bool,
    /// bind for the Massa gRPC API
    pub bind: SocketAddr,
    /// which compression encodings does the server accept for requests
    pub accept_compressed: Option<String>,
    /// which compression encodings might the server use for responses
    pub send_compressed: Option<String>,
    /// limits the maximum size of a decoded message. Defaults to 4MB
    pub max_decoding_message_size: usize,
    /// limits the maximum size of an encoded message. Defaults to 4MB
    pub max_encoding_message_size: usize,
    /// limits the maximum size of streaming channel
    pub max_channel_size: usize,
    /// set the concurrency limit applied to on requests inbound per connection. Defaults to 32
    pub concurrency_limit_per_connection: usize,
    /// set a timeout on for all request handlers
    pub timeout: MassaTime,
    /// sets the SETTINGS_INITIAL_WINDOW_SIZE spec option for HTTP2 stream-level flow control. Default is 65,535
    pub initial_stream_window_size: Option<u32>,
    /// sets the max connection-level flow control for HTTP2. Default is 65,535
    pub initial_connection_window_size: Option<u32>,
    /// sets the SETTINGS_MAX_CONCURRENT_STREAMS spec option for HTTP2 connections. Default is no limit (`None`)
    pub max_concurrent_streams: Option<u32>,
    /// set whether TCP keepalive messages are enabled on accepted connections
    pub tcp_keepalive: Option<MassaTime>,
    /// set the value of `TCP_NODELAY` option for accepted connections. Enabled by default
    pub tcp_nodelay: bool,
    /// set whether HTTP2 Ping frames are enabled on accepted connections. Default is no HTTP2 keepalive (`None`)
    pub http2_keepalive_interval: Option<MassaTime>,
    /// sets a timeout for receiving an acknowledgement of the keepalive ping. Default is 20 seconds
    pub http2_keepalive_timeout: Option<MassaTime>,
    /// sets whether to use an adaptive flow control. Defaults to false
    pub http2_adaptive_window: Option<bool>,
    /// sets the maximum frame size to use for HTTP2(must be within 16,384 and 16,777,215). If not set, will default from underlying transport
    pub max_frame_size: Option<u32>,
    /// when looking for next draw we want to look at max `draw_lookahead_period_count`
    pub draw_lookahead_period_count: u64,
    /// max number of block ids that can be included in a single request
    pub max_block_ids_per_request: u32,
}

#[cfg(test)]
#[test]
fn test_load_node_config() {
    let _ = *SETTINGS;
}
