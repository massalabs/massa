// Copyright (c) 2023 MASSA LABS <info@massa.net>

use massa_signature::KeyPair;
use massa_time::MassaTime;
use serde::Deserialize;
use std::{net::SocketAddr, path::PathBuf, time::Duration};

/// gRPC configuration.
/// the gRPC configuration
#[derive(Debug, Deserialize, Clone)]
pub struct GrpcConfig {
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
    /// set the concurrency limit applied to on requests inbound per connection. Defaults to 32
    pub concurrency_limit_per_connection: usize,
    /// set a timeout on for all request handlers
    pub timeout: Duration,
    /// sets the SETTINGS_INITIAL_WINDOW_SIZE spec option for HTTP2 stream-level flow control. Default is 65,535
    pub initial_stream_window_size: Option<u32>,
    /// sets the max connection-level flow control for HTTP2. Default is 65,535
    pub initial_connection_window_size: Option<u32>,
    /// sets the SETTINGS_MAX_CONCURRENT_STREAMS spec option for HTTP2 connections. Default is no limit (`None`)
    pub max_concurrent_streams: Option<u32>,
    /// max number of arguments per gRPC request
    pub max_arguments: u64,
    /// set whether TCP keepalive messages are enabled on accepted connections
    pub tcp_keepalive: Option<Duration>,
    /// set the value of `TCP_NODELAY` option for accepted connections. Enabled by default
    pub tcp_nodelay: bool,
    /// set whether HTTP2 Ping frames are enabled on accepted connections. Default is no HTTP2 keepalive (`None`)
    pub http2_keepalive_interval: Option<Duration>,
    /// sets a timeout for receiving an acknowledgement of the keepalive ping. Default is 20 seconds
    pub http2_keepalive_timeout: Option<Duration>,
    /// sets whether to use an adaptive flow control. Defaults to false
    pub http2_adaptive_window: Option<bool>,
    /// sets the maximum frame size to use for HTTP2. If not set, will default from underlying transport
    pub max_frame_size: Option<u32>,
    /// thread count
    pub thread_count: u8,
    /// max operations per block
    pub max_operations_per_block: u32,
    /// endorsement count
    pub endorsement_count: u32,
    /// max endorsements per message
    pub max_endorsements_per_message: u32,
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
    /// max operations per message in the network to avoid sending to big data packet
    pub max_operations_per_message: u32,
    /// `genesis_timestamp`
    pub genesis_timestamp: MassaTime,
    /// t0
    pub t0: MassaTime,
    /// periods per cycle
    pub periods_per_cycle: u64,
    /// keypair file
    pub keypair: KeyPair,
    /// limits the maximum size of streaming channel
    pub max_channel_size: usize,
    /// when looking for next draw we want to look at max `draw_lookahead_period_count`
    pub draw_lookahead_period_count: u64,
    /// last_start_period of the network, used to deserialize blocks
    pub last_start_period: u64,
    /// max denunciations in block header
    pub max_denunciations_per_block_header: u32,
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

/// gRPC API configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct GrpcApiConfig {
    /// Public server gRPC configuration.
    pub public: GrpcConfig,
    /// Private server gRPC configuration.
    pub private: GrpcConfig,
}
