// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::rpc::RpcClient;
use strum::{EnumMessage, IntoEnumIterator};
use strum_macros::{EnumIter, EnumMessage, EnumString, ToString};

#[derive(Debug, PartialEq, EnumIter, EnumMessage, EnumString, ToString)]
pub enum Command {
    #[strum(ascii_case_insensitive, message = "TODO")]
    Help,
    #[strum(ascii_case_insensitive, message = "TODO")]
    Unban,
}

// TODO: Commands could also not be APIs calls (like Wallet ones)
impl Command {
    // TODO: is Vec<String> -> String the best way to encode an User interaction in CLI?
    pub(crate) async fn run(&self, client: &RpcClient, parameters: &Vec<String>) -> String {
        match self {
            Command::Help => {
                if !parameters.is_empty() {
                    if let Ok(c) = parameters[0].parse::<Command>() {
                        return c.get_message().unwrap().to_string();
                    }
                }
                help();
                "".to_string() // FIXME: Ugly
            }
            // TODO: (de)serialize input/output from/to JSON with serde should be less verbose
            Command::Unban => serde_json::to_string(
                &client
                    .unban(serde_json::from_str(&parameters[0]).unwrap())
                    .await
                    .unwrap(), // FIXME: Better error handling ... (crash if server not running)
            )
            .unwrap(),
        }
    }
}

fn help() {
    println!("~~~~ HELP ~~~~");
    for c in Command::iter() {
        println!("{}", c.get_message().unwrap());
    }
}
