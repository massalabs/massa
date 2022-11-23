// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_time::MassaTime;

/// Http client settings.
/// the Http client settings
#[derive(Debug, Clone)]
pub struct HttpConfig {
    /// maximum size in bytes of a request.
    pub max_request_body_size: u32,
    /// maximum size in bytes of a response.
    pub request_timeout: MassaTime,
    /// maximum size in bytes of a response.
    pub max_concurrent_requests: usize,
    /// certificate_store, `Native` or `WebPki`
    pub certificate_store: String,
    /// JSON-RPC request object id data type, `Number` or `String`
    pub id_kind: String,
    /// max length for logging for requests and responses. Logs bigger than this limit will be truncated.
    pub max_log_length: u32,
    /// custom headers to pass with every request.
    pub headers: Vec<(String, String)>,
}
