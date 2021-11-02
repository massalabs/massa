// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::cfg::Settings;
use crate::cmds::Command;
use crate::rpc::Client;
use console::style;
use dialoguer::{theme::ColorfulTheme, Completion, History, Input};
use std::collections::VecDeque;
use strum::IntoEnumIterator;
use strum::ParseError;
use wallet::Wallet;

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
    println!();
    let mut history = MyHistory::default();
    let completion = MyCompletion::default();
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
                Ok(command) => println!("{}", command.run(client, wallet, &parameters).await),
                Err(_) => println!("{}", Command::not_found()),
            }
        }
    }
}

struct MyHistory {
    max: usize,
    history: VecDeque<String>,
}

impl Default for MyHistory {
    fn default() -> Self {
        let settings = Settings::load();
        MyHistory {
            max: settings.history,
            history: VecDeque::new(),
        }
    }
}

impl<T: ToString> History<T> for MyHistory {
    fn read(&self, pos: usize) -> Option<String> {
        self.history.get(pos).cloned()
    }

    fn write(&mut self, val: &T) {
        if self.history.len() == self.max {
            self.history.pop_back();
        }
        self.history.push_front(val.to_string());
    }
}

struct MyCompletion {
    options: Vec<String>,
}

impl Default for MyCompletion {
    fn default() -> Self {
        MyCompletion {
            options: Command::iter().map(|x| x.to_string()).collect(),
        }
    }
}

impl Completion for MyCompletion {
    /// Simple completion implementation based on substring
    fn get(&self, input: &str) -> Option<String> {
        let s = input.to_string();
        let ss: Vec<&String> = self
            .options
            .iter()
            .filter(|x| x.len() > s.len() && s == x[..s.len()])
            .collect();
        if ss.len() == 1 {
            Some(ss[0].to_string())
        } else {
            None
        }
    }
}
