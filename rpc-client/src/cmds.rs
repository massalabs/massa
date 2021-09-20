use crate::rpc::RpcClient;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, PartialEq)]
pub enum Command {
    HelloWorld, // REMOVE ME
    Unban,
    InteractiveMode,
}

impl FromStr for Command {
    type Err = String; // FIXME: create custom error type ...

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            // TODO: could a macro automate that? like `strum_macros::EnumString`
            "HelloWorld" => Ok(Command::HelloWorld),
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
    pub(crate) async fn run(&self, client: RpcClient, parameters: &str) -> String {
        match self {
            Command::HelloWorld => client.hello_world().await.unwrap(), // REMOVE ME
            // TODO: (de)serialize input/output from/to JSON with serde should be less verbose
            Command::Unban => serde_json::to_string(
                &client
                    .unban(serde_json::from_str(parameters).unwrap())
                    .await
                    .unwrap(),
            )
            .unwrap(),
            _ => panic!(),
        }
    }
}
