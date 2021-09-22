use crate::rpc::RpcClient;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, PartialEq)]
pub enum Command {
    InteractiveMode, // TODO: Should I keep this hack?
    Unban,
}

impl FromStr for Command {
    type Err = String; // FIXME: create custom error type ...

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            // TODO: could a macro automate that? like `strum_macros::EnumString`
            "InteractiveMode" => Ok(Command::InteractiveMode),
            "Unban" => Ok(Command::Unban),
            _ => Err("Command not found!".parse().unwrap()),
        }
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

// TODO: Commands could also not be APIs calls (like Wallet ones)
impl Command {
    // TODO: is Vec<String> -> String the best way to encode an User interaction in CLI?
    pub(crate) async fn run(&self, client: &RpcClient, parameters: &Vec<String>) -> String {
        match self {
            // TODO: (de)serialize input/output from/to JSON with serde should be less verbose
            Command::Unban => serde_json::to_string(
                &client
                    .unban(serde_json::from_str(&parameters[0]).unwrap())
                    .await
                    .unwrap(),
            )
            .unwrap(),
            _ => panic!(), // Should never happen so keep this panic!
                           // FIXME: InteractiveMode command in interactive mode trigger it ...
        }
    }
}
