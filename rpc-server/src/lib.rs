// Copyright (c) 2021 MASSA LABS <info@massa.net>

use communication::network::NetworkCommandSender;
use consensus::ConsensusCommandSender;
use pool::PoolCommandSender;

#[derive(Clone)]
pub struct API {
    pub url: String,
    pub pool_command_sender: Option<PoolCommandSender>,
    pub consensus_command_sender: Option<ConsensusCommandSender>,
    pub network_command_sender: Option<NetworkCommandSender>,
}

impl API {
    /// Default constructor that make all command senders fields to None
    pub fn from_url(url: &str) -> API {
        API {
            url: url.to_string(),
            pool_command_sender: None,
            consensus_command_sender: None,
            network_command_sender: None,
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
