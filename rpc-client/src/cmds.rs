// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::rpc::Client;
use console::style;
use std::process;
use strum::{EnumMessage, IntoEnumIterator};
use strum_macros::{EnumIter, EnumMessage, EnumString, ToString};

#[derive(Debug, PartialEq, EnumIter, EnumMessage, EnumString, ToString)]
pub enum Command {
    #[strum(ascii_case_insensitive, message = "TODO")]
    Exit,
    #[strum(ascii_case_insensitive, message = "TODO")]
    Help,
    #[strum(ascii_case_insensitive, message = "TODO")]
    Unban,
}

macro_rules! repl_error {
    ($err: expr) => {
        style(format!("Error: {}", $err)).red().to_string()
    };
}

// TODO: Commands could also not be APIs calls (like Wallet ones)
impl Command {
    pub(crate) fn not_found() {
        println!(
            "{}",
            repl_error!("Command not found!\ntype \"help\" to get the list of commands")
        );
    }

    // TODO: should run(...) be impl on Command or on some struct containing clients?
    pub(crate) async fn run(&self, client: &Client, parameters: &Vec<String>) {
        match self {
            Command::Exit => process::exit(0),
            Command::Help => {
                if !parameters.is_empty() {
                    if let Ok(c) = parameters[0].parse::<Command>() {
                        println!("{}", c.get_message().unwrap());
                    } else {
                        Command::not_found();
                    }
                } else {
                    help();
                }
            }
            Command::Unban => println!(
                "{}",
                // TODO: (de)serialize input/output from/to JSON with serde should be less verbose
                match serde_json::from_str(&parameters[0]) {
                    Ok(ip) => match &client.public.unban(ip).await {
                        Ok(output) => serde_json::to_string(output)
                            .expect("Failed to serialized command output ..."),
                        Err(e) => repl_error!(e),
                    },
                    Err(_) => repl_error!(
                        "IP given is not well formed...\ntype \"help unban\" to more info"
                    ),
                }
            ),
        }
    }
}

fn help() {
    println!("{}", style("~~~~ HELP ~~~~").green());
    for c in Command::iter() {
        println!("{}", c.get_message().unwrap());
    }
}
