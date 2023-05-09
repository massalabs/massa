// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_time::MassaTime;

/// Client common settings.
/// the client common settings
#[derive(Debug, Clone)]
pub(crate)  struct ClientConfig {
    /// maximum size in bytes of a request.
    pub(crate)  max_request_body_size: u32,
    /// maximum size in bytes of a response.
    pub(crate)  request_timeout: MassaTime,
    /// maximum size in bytes of a response.
    pub(crate)  max_concurrent_requests: usize,
    /// certificate_store, `Native` or `WebPki`
    pub(crate)  certificate_store: String,
    /// JSON-RPC request object id data type, `Number` or `String`
    pub(crate)  id_kind: String,
    /// max length for logging for requests and responses. Logs bigger than this limit will be truncated.
    pub(crate)  max_log_length: u32,
    /// custom headers to pass with every request.
    pub(crate)  headers: Vec<(String, String)>,
}

/// Http client settings.
/// the Http client settings
#[derive(Debug, Clone)]
pub(crate)  struct HttpConfig {
    /// common client configuration.
    pub(crate)  client_config: ClientConfig,
    /// whether to enable HTTP.
    pub(crate)  enabled: bool,
}

/// WebSocket client settings.
/// the WebSocket client settings
#[derive(Debug, Clone)]
pub(crate)  struct WsConfig {
    /// common client configuration.
    pub(crate)  client_config: ClientConfig,
    /// whether to enable WS.
    pub(crate)  enabled: bool,
    /// Max notifications per subscription.
    pub(crate)  max_notifs_per_subscription: usize,
    /// Max number of redirections.
    pub(crate)  max_redirections: usize,
}
