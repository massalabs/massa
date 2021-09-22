use crate::cmds::Command;
use crate::rpc::RpcClient;
use console::style;
use dialoguer::Input;

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
    loop {
        let input: Result<Command, String> = // FIXME: Use proper error type
            Input::<String>::new().interact_text().unwrap().parse();
        match input {
            Ok(command) => {
                println!("{}", command.run(client, parameters).await);
            }
            Err(err) => {
                println!("{}", err);
            }
        }
    }
}
