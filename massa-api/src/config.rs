// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_time::MassaTime;
use std::net::SocketAddr;
use std::path::PathBuf;

use serde::Deserialize;

/// API settings.
/// the API settings
#[derive(Debug, Deserialize, Clone)]
pub struct APIConfig {
    /// when looking for next draw we want to look at max `draw_lookahead_period_count`
    pub draw_lookahead_period_count: u64,
    /// bind for the private API
    pub bind_private: SocketAddr,
    /// bind for the public API
    pub bind_public: SocketAddr,
    /// max argument count
    pub max_arguments: u64,
    /// openrpc specification path
    pub openrpc_spec_file: PathBuf,
    /// bootstrap whitelist path
    pub bootstrap_whitelist_file: PathBuf,
    /// bootstrap blacklist path
    pub bootstrap_blacklist_file: PathBuf,
    /// maximum size in bytes of a request.
    pub max_request_body_size: u32,
    /// maximum size in bytes of a response.
    pub max_response_body_size: u32,
    /// maximum number of incoming connections allowed.
    pub max_connections: u32,
    /// maximum number of subscriptions per connection.
    pub max_subscriptions_per_connection: u32,
    /// max length for logging for requests and responses. Logs bigger than this limit will be truncated.
    pub max_log_length: u32,
    /// host filtering.
    pub allow_hosts: Vec<String>,
    /// whether batch requests are supported by this server or not.
    pub batch_requests_supported: bool,
    /// the interval at which `Ping` frames are submitted.
    pub ping_interval: MassaTime,
    /// whether to enable HTTP.
    pub enable_http: bool,
    /// whether to enable WS.
    pub enable_ws: bool,
    /// max datastore value length
    pub max_datastore_value_length: u64,
    /// max op datastore entry
    pub max_op_datastore_entry_count: u64,
    /// max datastore key length
    pub max_op_datastore_key_length: u8,
    /// max datastore value length
    pub max_op_datastore_value_length: u64,
    /// max function name length
    pub max_function_name_length: u16,
    /// max parameter size
    pub max_parameter_size: u32,
    /// thread count
    pub thread_count: u8,
    /// `genesis_timestamp`
    pub genesis_timestamp: MassaTime,
    /// t0
    pub t0: MassaTime,
    /// periods per cycle
    pub periods_per_cycle: u64,
}
