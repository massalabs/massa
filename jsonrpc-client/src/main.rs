use atty::Stream;
use jsonrpc_core_client::transports::http;
use jsonrpc_core_client::{RpcChannel, RpcResult, TypedClient};
use structopt::StructOpt;

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

#[derive(StructOpt)]
struct Args {
    /// Port to listen on.
    #[structopt(short = "p", long = "port", env = "PORT", default_value = "33035")]
    port: u16,
    /// Address to listen on.
    #[structopt(short = "a", long = "address", default_value = "127.0.0.1")]
    address: String,
}

#[paw::main]
fn main(args: Args) {
    if atty::is(Stream::Stdout) {
        // TODO: non-interactive mode
    } else {
        // TODO: interactive mode
    }
    let url = format!("http://{}:{}", args.address, args.port);
    let res = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let client = http::connect::<RpcClient>(&url).await.unwrap();
            client.hello_world().await.unwrap()
        });
    println!("{}", res);
}
