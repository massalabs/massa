// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::ask_password;
use crate::cmds::{Command, ExtendedWallet};
use crate::settings::SETTINGS;
use anyhow::Result;
use console::style;
use erased_serde::{Serialize, Serializer};
use massa_api_exports::{
    address::AddressInfo, block::BlockInfo, datastore::DatastoreEntryOutput,
    endorsement::EndorsementInfo, execution::ExecuteReadOnlyResponse, node::NodeStatus,
    operation::OperationInfo,
};
use massa_models::composite::PubkeySig;
use massa_models::output_event::SCOutputEvent;
use massa_models::prehash::PreHashSet;
use massa_models::{address::Address, operation::OperationId};
use massa_sdk::Client;
use massa_signature::{KeyPair, PublicKey};
use massa_wallet::Wallet;
use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::error::ReadlineError;
use rustyline::validate::MatchingBracketValidator;
use rustyline::{CompletionType, Config, Editor};
use rustyline_derive::{Completer, Helper, Highlighter, Hinter, Validator};
use std::env;
use std::net::IpAddr;
use std::path::Path;
use std::str;
use strum::IntoEnumIterator;
use strum::ParseError;

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

fn group_parameters(parameters: Vec<String>) -> Vec<String> {
    let mut new_parameters = Vec::new();
    let mut has_opening_simple_quote = false;
    let mut temp_simple_quote = String::new();
    let mut has_opening_double_quote = false;
    let mut temp_double_quote = String::new();
    for param in parameters {
        let mut chars = param.chars();
        match chars.next() {
            Some('\'') if !has_opening_double_quote => {
                has_opening_simple_quote = true;
                temp_simple_quote = param.clone();
                temp_simple_quote.remove(0);
            }
            Some('"') if !has_opening_simple_quote => {
                has_opening_double_quote = true;
                temp_double_quote = param.clone();
                temp_double_quote.remove(0);
            }
            Some(_) if has_opening_simple_quote => {
                temp_simple_quote.push(' ');
                temp_simple_quote.push_str(&param);
            }
            Some(_) if has_opening_double_quote => {
                temp_double_quote.push(' ');
                temp_double_quote.push_str(&param);
            }
            Some(_) => new_parameters.push(param.clone()),
            None => continue,
        };
        match chars.last() {
            Some('\'') if has_opening_simple_quote => {
                has_opening_simple_quote = false;
                let mut to_add = temp_simple_quote.clone();
                to_add.pop();
                new_parameters.push(to_add);
            }
            Some('"') if has_opening_double_quote => {
                has_opening_double_quote = false;
                let mut to_add = temp_double_quote.clone();
                to_add.pop();
                new_parameters.push(to_add);
            }
            Some(_) => continue,
            None => continue,
        }
    }
    new_parameters
}

#[derive(Helper, Completer, Hinter, Validator, Highlighter)]
struct MyHelper {
    #[rustyline(Completer)]
    completer: MassaCompleter,
    #[rustyline(Validator)]
    validator: MatchingBracketValidator,
}

pub(crate) async fn run(
    client: &Client,
    wallet_path: &Path,
    args_password: Option<String>,
) -> Result<()> {
    massa_fancy_ascii_art_logo!();
    println!("Use 'exit' or 'CTRL+D or CTRL+C' to quit the prompt");
    println!("Use the Up/Down arrows to scroll through history");
    println!("Use the Right arrow or Tab to complete your command");
    println!("Use the Enter key to execute your command");
    crate::cmds::help();
    let h = MyHelper {
        completer: MassaCompleter::new(),
        validator: MatchingBracketValidator::new(),
    };
    let config = Config::builder()
        .auto_add_history(true)
        .completion_prompt_limit(100)
        .completion_type(CompletionType::List)
        .max_history_size(10000)
        .build();
    let mut rl: Editor<MyHelper> = Editor::with_config(config)?;
    rl.set_helper(Some(h));
    if rl.load_history(&SETTINGS.history_file_path).is_err() {
        println!("No previous history.");
    }

    let mut wallet_opt = None;

    loop {
        let readline = rl.readline("command > ");
        match readline {
            Ok(line) => {
                if line.is_empty() {
                    continue;
                }
                rl.add_history_entry(line.as_str());
                let input: Vec<String> =
                    group_parameters(line.split_whitespace().map(|x| x.to_string()).collect());
                let cmd: Result<Command, ParseError> = input[0].parse();
                let parameters = input[1..].to_vec();
                // Print result of evaluated command
                match cmd {
                    Ok(command) => {
                        // Check if we need to prompt the user for their wallet password
                        if command.is_pwd_needed() && wallet_opt.is_none() {
                            let password =
                                match (args_password.clone(), env::var("MASSA_CLIENT_PASSWORD")) {
                                    (Some(pwd), _) => pwd,
                                    (_, Ok(pwd)) => pwd,
                                    _ => ask_password(wallet_path),
                                };

                            let wallet = Wallet::new(wallet_path.to_path_buf(), password)?;
                            wallet_opt = Some(wallet);
                        }

                        match command
                            .run(client, &mut wallet_opt, &parameters, false)
                            .await
                        {
                            Ok(output) => output.pretty_print(),
                            Err(e) => println!("{}", style(format!("Error: {}", e)).red()),
                        }
                    }
                    Err(_) => {
                        println!("Command not found!\ntype \"help\" to get the list of commands")
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                break;
            }
            Err(ReadlineError::Eof) => {
                break;
            }
            Err(err) => {
                println!("Error: {err:?}");
                break;
            }
        }
    }
    rl.append_history(&SETTINGS.history_file_path).unwrap();
    Ok(())
}

struct MassaCompleter {
    file_completer: FilenameCompleter,
}

impl MassaCompleter {
    fn new() -> Self {
        Self {
            file_completer: FilenameCompleter::new(),
        }
    }
}

impl Completer for MassaCompleter {
    type Candidate = Pair;
    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
        if line.contains(' ') {
            self.file_completer.complete(line, pos, ctx)
        } else {
            let mut candidates = Vec::new();
            for cmd in Command::iter() {
                if cmd.to_string().starts_with(line) {
                    candidates.push(Pair {
                        display: cmd.to_string(),
                        replacement: cmd.to_string(),
                    });
                }
            }
            Ok((0, candidates))
        }
    }
}

pub enum Style {
    Id,
    Pending,
    Finished,
    Good,
    Bad,
    Unknown,
    Block,
    Signature,
    Address,
    Fee,
}

impl Style {
    fn style<T: ToString>(&self, msg: T) -> console::StyledObject<std::string::String> {
        style(msg.to_string()).color256(match self {
            Style::Id => 175,
            Style::Pending => 172,
            Style::Finished => 105,
            Style::Good => 118,
            Style::Bad => 160,
            Style::Unknown => 248,
            Style::Block => 158,
            Style::Signature => 220,
            Style::Address => 147,
            Style::Fee => 55,
        })
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

impl Output for Vec<(Address, PublicKey)> {
    fn pretty_print(&self) {
        match self.len() {
            1 => println!("{}", self[0].1),
            _ => {
                for address_pubkey in self {
                    println!("Address: {}", address_pubkey.0);
                    println!("Public key: {}", address_pubkey.1);
                    println!();
                }
            }
        }
    }
}

impl Output for Vec<(Address, KeyPair)> {
    fn pretty_print(&self) {
        match self.len() {
            1 => println!("{}", self[0].1),
            _ => {
                for address_seckey in self {
                    println!("Address: {}", address_seckey.0);
                    println!("Secret key: {}", address_seckey.1);
                    println!();
                }
            }
        }
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

impl Output for PreHashSet<Address> {
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

impl Output for Vec<DatastoreEntryOutput> {
    fn pretty_print(&self) {
        for data_entry in self {
            println!("{}", data_entry);
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

impl Output for Vec<IpAddr> {
    fn pretty_print(&self) {
        for ips in self {
            println!("{}", ips);
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

impl Output for Vec<BlockInfo> {
    fn pretty_print(&self) {
        for block_info in self {
            println!("{}", block_info);
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

impl Output for Vec<SCOutputEvent> {
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
