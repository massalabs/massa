use crate::rpc::RpcClient;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, PartialEq)]
pub enum Command {
    Help,
    Unban,
}

impl FromStr for Command {
    type Err = String; // FIXME: create custom error type ...

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            // TODO: could a macro automate that? like `strum_macros::EnumString`
            "Help" => Ok(Command::Help),
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
            Command::Help => if let Ok(command) = parameters[0].parse::<Command>() {
                command.help()
            } else {
                "~~~~ HELP ~~~~"
            }
            .parse()
            .unwrap(),
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

    fn help(&self) -> &'static str {
        match self {
            Command::Help => "TODO",
            Command::Unban => "TODO",
        }
    }
}
