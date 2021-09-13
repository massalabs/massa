use atty::Stream;
use jsonrpc_core_client::transports::http;
use jsonrpc_core_client::{RpcChannel, RpcResult, TypedClient};

const URL: &str = "http://127.0.0.1:33035";

struct RpcClient(TypedClient);

impl From<RpcChannel> for RpcClient {
    fn from(channel: RpcChannel) -> Self {
        RpcClient(channel.into())
    }
}

impl RpcClient {
    async fn hello_world(&self) -> RpcResult<String> {
        self.0.call_method("HelloWorld", "String", ()).await
    }
}

#[tokio::main]
async fn main() {
    if atty::is(Stream::Stdout) {
        // TODO: non-interactive mode
    } else {
        // TODO: interactive mode
    }
    let client = http::connect::<RpcClient>(URL).await.unwrap();
    println!("{}", client.hello_world().await.unwrap());
}
