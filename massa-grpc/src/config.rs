// Copyright (c) 2023 MASSA LABS <info@massa.net>

use massa_time::MassaTime;
use serde::Deserialize;
use std::{net::SocketAddr, time::Duration};

/// gRPC configuration.
/// the gRPC configuration
#[derive(Debug, Deserialize, Clone)]
pub(crate)  struct GrpcConfig {
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
    /// set the concurrency limit applied to on requests inbound per connection. Defaults to 32
    pub(crate)  concurrency_limit_per_connection: usize,
    /// set a timeout on for all request handlers
    pub(crate)  timeout: Duration,
    /// sets the SETTINGS_INITIAL_WINDOW_SIZE spec option for HTTP2 stream-level flow control. Default is 65,535
    pub(crate)  initial_stream_window_size: Option<u32>,
    /// sets the max connection-level flow control for HTTP2. Default is 65,535
    pub(crate)  initial_connection_window_size: Option<u32>,
    /// sets the SETTINGS_MAX_CONCURRENT_STREAMS spec option for HTTP2 connections. Default is no limit (`None`)
    pub(crate)  max_concurrent_streams: Option<u32>,
    /// set whether TCP keepalive messages are enabled on accepted connections
    pub(crate)  tcp_keepalive: Option<Duration>,
    /// set the value of `TCP_NODELAY` option for accepted connections. Enabled by default
    pub(crate)  tcp_nodelay: bool,
    /// set whether HTTP2 Ping frames are enabled on accepted connections. Default is no HTTP2 keepalive (`None`)
    pub(crate)  http2_keepalive_interval: Option<Duration>,
    /// sets a timeout for receiving an acknowledgement of the keepalive ping. Default is 20 seconds
    pub(crate)  http2_keepalive_timeout: Option<Duration>,
    /// sets whether to use an adaptive flow control. Defaults to false
    pub(crate)  http2_adaptive_window: Option<bool>,
    /// sets the maximum frame size to use for HTTP2. If not set, will default from underlying transport
    pub(crate)  max_frame_size: Option<u32>,
    /// thread count
    pub(crate)  thread_count: u8,
    /// max operations per block
    pub(crate)  max_operations_per_block: u32,
    /// endorsement count
    pub(crate)  endorsement_count: u32,
    /// max endorsements per message
    pub(crate)  max_endorsements_per_message: u32,
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
    /// max operations per message in the network to avoid sending to big data packet
    pub(crate)  max_operations_per_message: u32,
    /// `genesis_timestamp`
    pub(crate)  genesis_timestamp: MassaTime,
    /// t0
    pub(crate)  t0: MassaTime,
    /// periods per cycle
    pub(crate)  periods_per_cycle: u64,
    /// limits the maximum size of streaming channel
    pub(crate)  max_channel_size: usize,
    /// when looking for next draw we want to look at max `draw_lookahead_period_count`
    pub(crate)  draw_lookahead_period_count: u64,
    /// last_start_period of the network, used to deserialize blocks
    pub(crate)  last_start_period: u64,
    /// max denunciations in block header
    pub(crate)  max_denunciations_per_block_header: u32,
    /// max number of block ids that can be included in a single request
    pub(crate)  max_block_ids_per_request: u32,
    /// max number of operation ids that can be included in a single request
    pub(crate)  max_operation_ids_per_request: u32,
}
