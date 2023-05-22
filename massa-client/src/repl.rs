// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::ask_password;
use crate::cmds::Command;
use crate::massa_fancy_ascii_art_logo;
use crate::settings::SETTINGS;
use anyhow::Result;
use console::style;
use massa_sdk::Client;
use massa_wallet::Wallet;
use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::error::ReadlineError;
use rustyline::validate::MatchingBracketValidator;
use rustyline::{CompletionType, Config, Editor};
use rustyline_derive::{Completer, Helper, Highlighter, Hinter, Validator};
use std::env;
use std::path::Path;
use strum::IntoEnumIterator;
use strum::ParseError;

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
    client: &mut Client,
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
