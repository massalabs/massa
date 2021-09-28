// Copyright (c) 2021 MASSA LABS <info@massa.net>

use jsonrpc_core_client::transports::http;
use jsonrpc_core_client::{RpcChannel, RpcResult, TypedClient};
use std::net::IpAddr;

// TODO: This crate should at some point be renamed `client`, `massa` or `massa-client`
// and replace the previous one!

// TODO: Did we crate 2 RpcClient structs? (to separate public/private calls in impl)
pub struct RpcClient(TypedClient);

/// This is required by `jsonrpc_core_client::transports::http::connect`
impl From<RpcChannel> for RpcClient {
    fn from(channel: RpcChannel) -> Self {
        RpcClient(channel.into())
    }
}

/// Typed wrapper to API calls based on the method given by `jsonrpc_core_client`:
///
/// ```rust
/// fn call_method<T: Serialize, R: DeserializeOwned>(
///     method: &str,
///     returns: &str,
///     args: T,
/// ) -> impl Future<Output = RpcResult<R>> {
/// }
/// ```
impl RpcClient {
    /// Default constructor
    pub(crate) async fn from_url(url: &str) -> RpcClient {
        match http::connect::<RpcClient>(&url).await {
            Ok(client) => client,
            Err(_) => panic!("Unable to connect to Node."),
        }
    }

    /// End-to-end example with `Unban` command
    pub(crate) async fn unban(&self, ip: IpAddr) -> RpcResult<()> {
        self.0.call_method("Unban", "()", ip).await
    }

    // TODO: We should here implement all of our desired API calls
}
