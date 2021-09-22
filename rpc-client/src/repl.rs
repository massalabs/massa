use crate::cmds::Command;
use crate::rpc::RpcClient;
use console::style;
use dialoguer::{theme::ColorfulTheme, History, Input};
use std::{collections::VecDeque, fmt::Display, process};
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

pub(crate) async fn run(client: &RpcClient, parameters: &Vec<String>) {
    massa_fancy_ascii_art_logo!();
    println!("Use 'exit' to quit the prompt");
    println!("Use the Up/Down arrows to scroll through history");
    println!();
    let mut history = MyHistory::default();
    loop {
        if let Ok(cmd) = Input::<String>::with_theme(&ColorfulTheme::default())
            .with_prompt("command")
            .history_with(&mut history)
            .interact_text()
        {
            if cmd == "exit" {
                process::exit(0);
            }
            let input: Result<Command, ParseError> = cmd.parse();
            println!(
                "{}",
                match input {
                    Ok(command) => command.run(client, parameters).await,
                    Err(_) => "Command not found!".to_string(),
                }
            );
        }
    }
}

const HISTORY: usize = 10000; // TODO: Should be available as a CLI arg/into `config.toml`?

struct MyHistory {
    max: usize,
    history: VecDeque<String>,
}

impl Default for MyHistory {
    fn default() -> Self {
        MyHistory {
            max: HISTORY,
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
