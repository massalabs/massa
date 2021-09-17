// Copyright (c) 2021 MASSA LABS <info@massa.net>

use communication::network::NetworkCommandSender;

// TODO: share this structure between all api-* crates
#[derive(Clone)]
pub struct API {
    pub url: String,
    pub network_command_sender: Option<NetworkCommandSender>,
}

impl API {
    pub fn set_network_command_sender(&mut self, network_command_sender: NetworkCommandSender) {
        self.network_command_sender = Some(network_command_sender);
        // TODO: write a way to update all command senders
    }

    // TODO: write a default constructor `new` that make all command senders fields to None
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
