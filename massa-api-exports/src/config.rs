// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_signature::KeyPair;
use massa_time::MassaTime;
use std::net::SocketAddr;
use std::path::PathBuf;

use serde::Deserialize;

/// API settings.
/// the API settings
#[derive(Debug, Deserialize, Clone)]
pub(crate)  struct APIConfig {
    /// when looking for next draw we want to look at max `draw_lookahead_period_count`
    pub(crate)  draw_lookahead_period_count: u64,
    /// bind for the private API
    pub(crate)  bind_private: SocketAddr,
    /// bind for the public API
    pub(crate)  bind_public: SocketAddr,
    /// bind for the Massa API
    pub(crate)  bind_api: SocketAddr,
    /// max argument count
    pub(crate)  max_arguments: u64,
    /// openrpc specification path
    pub(crate)  openrpc_spec_path: PathBuf,
    /// bootstrap whitelist path
    pub(crate)  bootstrap_whitelist_path: PathBuf,
    /// bootstrap blacklist path
    pub(crate)  bootstrap_blacklist_path: PathBuf,
    /// maximum size in bytes of a request.
    pub(crate)  max_request_body_size: u32,
    /// maximum size in bytes of a response.
    pub(crate)  max_response_body_size: u32,
    /// maximum number of incoming connections allowed.
    pub(crate)  max_connections: u32,
    /// maximum number of subscriptions per connection.
    pub(crate)  max_subscriptions_per_connection: u32,
    /// max length for logging for requests and responses. Logs bigger than this limit will be truncated.
    pub(crate)  max_log_length: u32,
    /// host filtering.
    pub(crate)  allow_hosts: Vec<String>,
    /// whether batch requests are supported by this server or not.
    pub(crate)  batch_requests_supported: bool,
    /// the interval at which `Ping` frames are submitted.
    pub(crate)  ping_interval: MassaTime,
    /// whether to enable HTTP.
    pub(crate)  enable_http: bool,
    /// whether to enable WS.
    pub(crate)  enable_ws: bool,
    /// max datastore value length
    pub(crate)  max_datastore_value_length: u64,
    /// max op datastore entry
    pub(crate)  max_op_datastore_entry_count: u64,
    /// max datastore key length
    pub(crate)  max_op_datastore_key_length: u8,
    /// max datastore value length
    pub(crate)  max_op_datastore_value_length: u64,
    /// max function name length
    pub(crate)  max_function_name_length: u16,
    /// max parameter size
    pub(crate)  max_parameter_size: u32,
    /// thread count
    pub(crate)  thread_count: u8,
    /// `genesis_timestamp`
    pub(crate)  genesis_timestamp: MassaTime,
    /// t0
    pub(crate)  t0: MassaTime,
    /// periods per cycle
    pub(crate)  periods_per_cycle: u64,
    /// keypair file
    pub(crate)  keypair: KeyPair,
}
