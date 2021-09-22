// Copyright (c) 2021 MASSA LABS <info@massa.net>

use communication::network::NetworkCommandSender;
use config::APIConfig;
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

    /// A handy way to update all command senders
    pub fn set_command_senders(
        &mut self,
        pool_command_sender: Option<PoolCommandSender>,
        consensus_command_sender: Option<ConsensusCommandSender>,
        network_command_sender: Option<NetworkCommandSender>,
    ) {
        self.pool_command_sender = pool_command_sender;
        self.consensus_command_sender = consensus_command_sender;
        self.network_command_sender = network_command_sender;
    }

    pub fn set_configs(&mut self, consensus_config: ConsensusConfig, api: APIConfig) {
        self.consensus_config = Some(consensus_config);
        self.api_config = Some(api);
        // todo add needed configs
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
