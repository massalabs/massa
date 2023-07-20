// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Build here the default node settings from the configuration file toml
use std::{collections::HashMap, path::PathBuf};

use massa_bootstrap::IpType;
use massa_models::{config::build_massa_settings, node::NodeId};
use massa_protocol_exports::PeerCategoryInfo;
use massa_time::MassaTime;
use serde::Deserialize;
use std::net::{IpAddr, SocketAddr};

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
    /// slot execution outputs channel capacity
    pub broadcast_slot_execution_output_channel_capacity: usize,
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
    pub max_bytes_read_write: u64,
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
    /// stop the production in case we are not connected to anyone
    pub stop_production_when_zero_connections: bool,
}

/// Pool configuration, read from a file configuration
#[derive(Debug, Deserialize, Clone)]
pub struct PoolSettings {
    pub max_operation_pool_size: usize,
    pub max_operation_pool_excess_items: usize,
    pub operation_max_future_start_delay: MassaTime,
    pub operation_pool_refresh_interval: MassaTime,
    pub max_endorsements_pool_size_per_thread: usize,
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
    pub batch_request_limit: u32,
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
    pub consensus: ConsensusSettings,
    pub api: APISettings,
    pub network: NetworkSettings,
    pub bootstrap: BootstrapSettings,
    pub pool: PoolSettings,
    pub execution: ExecutionSettings,
    pub ledger: LedgerSettings,
    pub selector: SelectionSettings,
    pub factory: FactorySettings,
    pub grpc: GrpcApiSettings,
    pub metrics: MetricsSettings,
    pub versioning: VersioningSettings,
}

/// Consensus configuration
/// Assumes `thread_count >= 1, t0_millis >= 1, t0_millis % thread_count == 0`
#[derive(Debug, Deserialize, Clone)]
pub struct ConsensusSettings {
    /// Maximum number of blocks allowed in discarded blocks.
    pub max_discarded_blocks: usize,
    /// Maximum number of blocks allowed in `FutureIncomingBlocks`.
    pub max_future_processing_blocks: usize,
    /// Maximum number of blocks allowed in `DependencyWaitingBlocks`.
    pub max_dependency_blocks: usize,
    /// stats time span
    pub stats_timespan: MassaTime,
    /// force keep at least this number of final periods in RAM for each thread
    pub force_keep_final_periods: u64,
    /// force keep at least this number of final periods without operations in RAM for each thread
    pub force_keep_final_periods_without_ops: u64,
    /// old blocks are pruned every `block_db_prune_interval`
    pub block_db_prune_interval: MassaTime,
    /// blocks headers channel capacity
    pub broadcast_blocks_headers_channel_capacity: usize,
    /// blocks channel capacity
    pub broadcast_blocks_channel_capacity: usize,
    /// filled blocks channel capacity
    pub broadcast_filled_blocks_channel_capacity: usize,
}

// TODO: Remove one date. Kept for retro compatibility.
#[derive(Debug, Deserialize, Clone)]
pub struct NetworkSettings {
    /// Ip seen by others. If none the bind ip is used
    pub routable_ip: Option<IpAddr>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MetricsSettings {
    /// enable prometheus metrics
    pub enabled: bool,
    /// port on which to listen for prometheus metrics
    pub bind: SocketAddr,
    /// interval at which to update metrics
    pub tick_delay: MassaTime,
}

/// Protocol Configuration, read from toml user configuration file
#[derive(Debug, Deserialize, Clone)]
pub struct ProtocolSettings {
    /// after `ask_block_timeout` milliseconds we try to ask a block to another node
    pub ask_block_timeout: MassaTime,
    /// Max known blocks we keep during their propagation
    pub max_blocks_kept_for_propagation: usize,
    /// Time during which a block is expected to propagate
    pub max_block_propagation_time: MassaTime,
    /// Block propagation tick interval, useful for propagating blocks quickly to newly connected peers.
    pub block_propagation_tick: MassaTime,
    /// max known blocks our node keeps in its knowledge cache
    pub max_known_blocks_size: usize,
    /// max cache size for which blocks a foreign node knows about
    pub max_node_known_blocks_size: usize,
    /// max wanted blocks per node kept in memory
    pub max_node_wanted_blocks_size: usize,
    /// max known operations current node kept in memory
    pub max_known_ops_size: usize,
    /// size of the buffer of asked operations
    pub asked_operations_buffer_capacity: usize,
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
    /// Interval at which operations are announced in batches.
    pub operation_announcement_interval: MassaTime,
    /// Maximum of operations sent in one message.
    pub max_operations_per_message: u64,
    /// MAx number of operations kept for propagation
    pub max_ops_kept_for_propagation: usize,
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
    /// Time threshold to have a connection to a node
    pub connect_timeout: MassaTime,
    /// Number of tester threads
    pub thread_tester_count: u8,
    /// Number of bytes we can read/write by seconds in a connection (must be a 10 multiple)
    pub read_write_limit_bytes_per_second: u64,
    /// try connection timer
    pub try_connection_timer: MassaTime,
    /// try connection timer for the same peer
    pub try_connection_timer_same_peer: MassaTime,
    /// periodically unban every peer
    pub unban_everyone_timer: MassaTime,
    /// Timeout connection
    pub timeout_connection: MassaTime,
    /// Message timeout
    pub message_timeout: MassaTime,
    /// Timeout for the tester operations
    pub tester_timeout: MassaTime,
    /// Nb in connections
    pub max_in_connections: usize,
    /// Peers limits per category
    pub peers_categories: HashMap<String, PeerCategoryInfo>,
    /// Limits for default category
    pub default_category_info: PeerCategoryInfo,
    /// Cooldown before testing again an old peer
    pub test_oldest_peer_cooldown: MassaTime,
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
    /// whether to enable gRPC health service
    pub enable_health: bool,
    /// whether to enable gRPC reflection
    pub enable_reflection: bool,
    /// whether to enable TLS
    pub enable_tls: bool,
    /// whether to enable mTLS (requires `enable_tls` to be true)
    pub enable_mtls: bool,
    /// whether to generate a self-signed certificate if none is provided
    pub generate_self_signed_certificates: bool,
    /// whether to use the same certificate_authority for client and server certificates(requires `generate_self_signed_certificates` to be true)
    pub use_same_certificate_authority_for_client: bool,
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
    /// max number of arguments per gRPC request
    pub max_arguments: u64,
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
    /// max number of operation ids that can be included in a single request
    pub max_operation_ids_per_request: u32,
    /// server certificate path
    pub server_certificate_path: PathBuf,
    /// server private key path
    pub server_private_key_path: PathBuf,
    /// client certificate authority root path
    pub client_certificate_authority_root_path: PathBuf,
}

/// gRPC API settings.
#[derive(Debug, Deserialize, Clone)]
pub struct GrpcApiSettings {
    /// Public server gRPC configuration.
    pub public: GrpcSettings,
    /// Private server gRPC configuration.
    pub private: GrpcSettings,
}

#[derive(Debug, Deserialize, Clone)]
pub struct VersioningSettings {
    // Warn user to update its node if we reach this percentage for announced network versions
    pub(crate) mip_stats_warn_announced_version: u32,
}

#[cfg(test)]
#[test]
fn test_load_node_config() {
    let _ = *SETTINGS;
}
