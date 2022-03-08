// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::cmds::{Command, ExtendedWallet};
use crate::rpc::Client;
use crate::settings::SETTINGS;
use crate::utils::longest_common_prefix;
use console::style;
use dialoguer::{theme::ColorfulTheme, Completion, History, Input};
use erased_serde::{Serialize, Serializer};
use glob::glob;
use massa_models::api::{AddressInfo, BlockInfo, EndorsementInfo, NodeStatus, OperationInfo};
use massa_models::composite::PubkeySig;
use massa_models::execution::ExecuteReadOnlyResponse;
use massa_models::prehash::Set;
use massa_models::{Address, OperationId};
use massa_wallet::Wallet;
use rev_lines::RevLines;
use std::collections::VecDeque;
use std::io::Error;
use std::str;
use std::{
    fs::File,
    fs::OpenOptions,
    io::{BufReader, Write},
};
use strum::IntoEnumIterator;
use strum::ParseError;
#[cfg(not(windows))]
use tilde_expand::tilde_expand;

macro_rules! massa_fancy_ascii_art_logo {
    () => {
        println!(
            "{}\n{}\n{}\n{}\n{}\n",
            style("███    ███  █████  ███████ ███████  █████ ").color256(160),
            style("████  ████ ██   ██ ██      ██      ██   ██").color256(161),
            style("██ ████ ██ ███████ ███████ ███████ ███████").color256(162),
            style("██  ██  ██ ██   ██      ██      ██ ██   ██").color256(163),
            style("██      ██ ██   ██ ███████ ███████ ██   ██").color256(164)
        );
    };
}

pub(crate) async fn run(client: &Client, wallet: &mut Wallet) {
    massa_fancy_ascii_art_logo!();
    println!("Use 'exit' to quit the prompt");
    println!("Use the Up/Down arrows to scroll through history");
    println!("Use the Right arrow or Tab to complete your command");
    println!("Use the Enter key to execute your command");
    crate::cmds::help();
    let mut history = CommandHistory::default();
    let completion = CommandCompletion::default();
    loop {
        if let Ok(input) = Input::<String>::with_theme(&ColorfulTheme::default())
            .with_prompt("command")
            .history_with(&mut history)
            .completion_with(&completion)
            .interact_text()
        {
            // User input parsing
            let input: Vec<String> = input.split_whitespace().map(|x| x.to_string()).collect();
            let cmd: Result<Command, ParseError> = input[0].parse();
            let parameters = input[1..].to_vec();
            // Print result of evaluated command
            match cmd {
                Ok(command) => match command.run(client, wallet, &parameters, false).await {
                    Ok(output) => output.pretty_print(),
                    Err(e) => println!("{}", style(format!("Error: {}", e)).red()),
                },
                Err(_) => println!("Command not found!\ntype \"help\" to get the list of commands"),
            }
        }
    }
}

struct CommandHistory {
    max: usize,
    history: VecDeque<String>,
}

impl CommandHistory {
    fn get_saved_history() -> Result<VecDeque<String>, Error> {
        if let Ok(file) = File::open(&SETTINGS.history_file_path) {
            let lines = RevLines::new(BufReader::new(file))?;
            Ok(lines.collect())
        } else {
            File::create(&SETTINGS.history_file_path)?;
            Ok(VecDeque::new())
        }
    }

    fn write_to_saved_history(command: &str) {
        if let Ok(mut file) = OpenOptions::new()
            .write(true)
            .append(true)
            .open(&SETTINGS.history_file_path)
        {
            writeln!(file, "{}", command).ok();
        }
    }
}

impl Default for CommandHistory {
    fn default() -> Self {
        CommandHistory {
            max: SETTINGS.history,
            history: CommandHistory::get_saved_history().unwrap_or_default(),
        }
    }
}

impl<T: ToString> History<T> for CommandHistory {
    fn read(&self, pos: usize) -> Option<String> {
        self.history.get(pos).cloned()
    }

    fn write(&mut self, val: &T) {
        if self.history.len() == self.max {
            self.history.pop_back();
        }
        let string_value = val.to_string();
        CommandHistory::write_to_saved_history(&string_value);
        self.history.push_front(string_value);
    }
}

struct CommandCompletion {
    options: Vec<String>,
}

impl Default for CommandCompletion {
    fn default() -> Self {
        CommandCompletion {
            options: Command::iter().map(|x| x.to_string()).collect(),
        }
    }
}

#[cfg(not(windows))]
fn expand_path(partial_path: &str) -> Vec<u8> {
    tilde_expand(partial_path.as_bytes())
}

#[cfg(windows)]
fn expand_path(partial_path: &str) -> Vec<u8> {
    partial_path.as_bytes().to_vec()
}

impl Completion for CommandCompletion {
    /// Simple completion implementation based on substring
    fn get(&self, input: &str) -> Option<String> {
        let input = input.to_string();
        if input.contains(' ') {
            let mut args: Vec<&str> = input.split(' ').collect();
            let mut default_path = "./";
            let path_to_complete = args.last_mut().unwrap_or(&mut default_path);
            let expanded_path = expand_path(path_to_complete);
            *path_to_complete = str::from_utf8(&expanded_path).unwrap_or(path_to_complete);
            if let Ok(paths) = glob(&(path_to_complete.to_owned() + "*")) {
                let suggestions: Vec<String> = paths
                    .filter_map(|x| x.map(|path| path.display().to_string()).ok())
                    .collect();
                if !suggestions.is_empty() {
                    println!();
                    for path in &suggestions {
                        println!("{}", style(path).dim())
                    }
                    *path_to_complete =
                        longest_common_prefix(suggestions.iter().map(|s| &s[..]).collect());
                }
                Some(args.join(" "))
            } else {
                Some(args.join(" "))
            }
        } else {
            let suggestions: Vec<&str> = self
                .options
                .iter()
                .filter(|s| s.len() >= input.len() && input == s[..input.len()])
                .map(|s| &s[..])
                .collect();
            if !suggestions.is_empty() {
                println!();
                for suggestion in &suggestions {
                    println!("{}", style(suggestion).dim());
                }
                Some(String::from(longest_common_prefix(suggestions)))
            } else {
                None
            }
        }
    }
}

pub trait Output: Serialize {
    fn pretty_print(&self);
}

impl dyn Output {
    pub(crate) fn stdout_json(&self) -> anyhow::Result<()> {
        let json = &mut serde_json::Serializer::new(std::io::stdout());
        let mut format: Box<dyn Serializer> = Box::new(<dyn Serializer>::erase(json));
        self.erased_serialize(&mut format)?;
        Ok(())
    }
}

impl Output for Wallet {
    fn pretty_print(&self) {
        println!("{}", self);
    }
}

impl Output for ExtendedWallet {
    fn pretty_print(&self) {
        println!("{}", self);
    }
}

impl Output for () {
    fn pretty_print(&self) {}
}

impl Output for String {
    fn pretty_print(&self) {
        println!("{}", self);
    }
}

impl Output for &str {
    fn pretty_print(&self) {
        println!("{}", self)
    }
}

impl Output for NodeStatus {
    fn pretty_print(&self) {
        println!("{}", self);
    }
}

impl Output for BlockInfo {
    fn pretty_print(&self) {
        println!("{}", self);
    }
}

impl Output for Set<Address> {
    fn pretty_print(&self) {
        println!(
            "{}",
            self.iter()
                .fold("".to_string(), |acc, a| format!("{}{}\n", acc, a))
        )
    }
}

impl Output for Vec<AddressInfo> {
    fn pretty_print(&self) {
        for address_info in self {
            println!("{}", address_info);
        }
    }
}

impl Output for Vec<EndorsementInfo> {
    fn pretty_print(&self) {
        for endorsement_info in self {
            println!("{}", endorsement_info);
        }
    }
}

impl Output for Vec<OperationInfo> {
    fn pretty_print(&self) {
        for operation_info in self {
            println!("{}", operation_info);
        }
    }
}

impl Output for Vec<OperationId> {
    fn pretty_print(&self) {
        for operation_id in self {
            println!("{}", operation_id);
        }
    }
}

impl Output for Vec<Address> {
    fn pretty_print(&self) {
        for addr in self {
            println!("{}", addr);
        }
    }
}

impl Output for PubkeySig {
    fn pretty_print(&self) {
        println!("{}", self);
    }
}

impl Output for ExecuteReadOnlyResponse {
    fn pretty_print(&self) {
        println!("{}", self);
    }
}
