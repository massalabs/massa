// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::cfg::Config;
use crate::cmds::Command;
use crate::rpc::RpcClient;
use console::style;
use dialoguer::{theme::ColorfulTheme, History, Input};
use std::{collections::VecDeque, fmt::Display};
use strum::ParseError;

macro_rules! massa_fancy_ascii_art_logo {
    () => {
        println!(
            "{}\n{}\n{}\n{}\n{}\n",
            style("███    ███  █████  ███████ ███████  █████").color256(160),
            style("████  ████ ██   ██ ██      ██      ██   ██").color256(161),
            style("██ ████ ██ ███████ ███████ ███████ ███████").color256(162),
            style("██  ██  ██ ██   ██      ██      ██ ██   ██").color256(163),
            style("██      ██ ██   ██ ███████ ███████ ██   ██").color256(164)
        );
    };
}

pub(crate) async fn run(public_client: &RpcClient, private_client: &RpcClient) {
    massa_fancy_ascii_art_logo!();
    println!("Use 'exit' to quit the prompt");
    println!("Use the Up/Down arrows to scroll through history");
    println!();
    let mut history = MyHistory::default();
    loop {
        if let Ok(input) = Input::<String>::with_theme(&ColorfulTheme::default())
            .with_prompt("command")
            .history_with(&mut history)
            .interact_text()
        {
            // User input parsing
            let input: Vec<String> = input.split_whitespace().map(|x| x.to_string()).collect();
            let cmd: Result<Command, ParseError> = input[0].parse();
            let parameters = input[1..].to_vec();
            // Print result of evaluated command
            match cmd {
                Ok(command) => {
                    command
                        .run(public_client, private_client, &parameters)
                        .await
                }
                Err(_) => Command::not_found(),
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
        // FIXME: Duplicated code load config
        let config_path = "base_config/config.toml";
        let override_config_path = "config/config.toml";
        let mut cfg = config::Config::default();
        cfg.merge(config::File::with_name(config_path))
            .expect("could not load main config file");
        if std::path::Path::new(override_config_path).is_file() {
            cfg.merge(config::File::with_name(override_config_path))
                .expect("could not load override config file");
        }
        let cfg = cfg.try_into::<Config>().expect("error structuring config");

        MyHistory {
            max: cfg.history,
            history: VecDeque::new(),
        }
    }
}

impl<T> History<T> for MyHistory {
    fn read(&self, pos: usize) -> Option<String> {
        self.history.get(pos).cloned()
    }

    fn write(&mut self, val: &T)
    where
        T: Clone + Display,
    {
        if self.history.len() == self.max {
            self.history.pop_back();
        }
        self.history.push_front(val.to_string());
    }
}
