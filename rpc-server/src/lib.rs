// Copyright (c) 2021 MASSA LABS <info@massa.net>

use communication::network::NetworkCommandSender;
pub use config::APIConfig;
use consensus::{ConsensusCommandSender, ConsensusConfig};
use pool::PoolCommandSender;

mod config;
#[derive(Clone)]
pub struct API {
    pub url: String,
    pub pool_command_sender: Option<PoolCommandSender>,
    pub consensus_command_sender: Option<ConsensusCommandSender>,
    pub network_command_sender: Option<NetworkCommandSender>,
    pub consensus_config: Option<ConsensusConfig>,
    pub api_config: Option<APIConfig>,
}

impl API {
    /// Default constructor that make all command senders fields to None
    pub fn from_url(url: &str) -> API {
        API {
            url: url.to_string(),
            pool_command_sender: None,
            consensus_command_sender: None,
            network_command_sender: None,
            consensus_config: None,
            api_config: None,
        }
    }
}

#[macro_export]
macro_rules! rpc_server {
    ($api:expr) => {
        let mut io = IoHandler::new();
        io.extend_with($api.clone().to_delegate());

        let server = ServerBuilder::new(io)
            .start_http(&$api.url.parse().unwrap())
            .expect("Unable to start RPC server");

        thread::spawn(|| server.wait())
    };
}
