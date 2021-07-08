use crate::repl::helper::HISTORY_FILE;
use core::convert::TryFrom;
use rustyline::error::ReadlineError;
use rustyline::{CompletionType, Config, Editor};
use std::net::SocketAddr;

pub mod error;
mod helper;

pub struct ReplData {
    pub node_ip: SocketAddr,
}
impl Default for ReplData {
    fn default() -> Self {
        ReplData {
            node_ip: "0.0.0.0:3030".parse().unwrap(),
        }
    }
}

pub type CmdFn = Box<dyn Fn(&mut ReplData, &[&str]) -> Result<(), error::ReplError> + Send + Sync>;

pub struct Command {
    name: String,
    max_nb_param: usize,
    help: String,
    func: CmdFn,
}

struct TypedCommand<'a> {
    name: String,
    params: Vec<&'a str>,
}

impl<'a> TryFrom<&'a str> for TypedCommand<'a> {
    type Error = error::ReplError;

    fn try_from(line: &'a str) -> Result<Self, Self::Error> {
        let mut iter = line.split(' ');
        let name = iter
            .next()
            .ok_or(error::ReplError::ParseCommandError)?
            .to_string();
        let params: Vec<&str> = iter.collect();
        Ok(TypedCommand { name, params })
    }
}

pub struct Repl {
    pub data: ReplData,
    cmd_list: Vec<Command>,
}

impl Repl {
    pub fn new() -> Self {
        let mut repl = Repl {
            data: Default::default(),
            cmd_list: Vec::new(),
        };
        repl.new_command_noargs("quit", "quit Massa client", quit_func);
        repl.new_command_noargs("help", "this help", help_func);
        repl.new_command_noargs("", "", empty_cmd_func);
        repl
    }

    pub fn new_command_noargs<S, F>(&mut self, name: S, help: S, func: F)
    where
        S: ToString,
        F: Fn(&mut ReplData, &[&str]) -> Result<(), error::ReplError> + Send + Sync + 'static,
    {
        self.cmd_list.push(Command {
            name: name.to_string(),
            max_nb_param: 0,
            help: help.to_string(),
            func: Box::new(func),
        });
    }

    //max_nb_param is the max number of parameters that the command can take.
    pub fn new_command<S, F>(&mut self, name: S, help: S, max_nb_param: usize, func: F)
    where
        S: ToString,
        F: Fn(&mut ReplData, &[&str]) -> Result<(), error::ReplError> + Send + Sync + 'static,
    {
        self.cmd_list.push(Command {
            name: name.to_string(),
            max_nb_param,
            help: help.to_string(),
            func: Box::new(func),
        });
    }

    pub fn run(mut self) -> Result<(), error::ReplError> {
        let mut helper = helper::ReplHelper::new();
        let config = Config::builder()
            //.completion_type(CompletionType::Circular)
            .completion_type(CompletionType::List)
            .build();

        //declare all cmd
        self.cmd_list
            .iter()
            .for_each(|cmd| helper.add_cmd(cmd.name.clone(), cmd.max_nb_param));

        let mut rl = Editor::with_config(config);
        rl.set_helper(Some(helper));

        if rl.load_history(HISTORY_FILE).is_err() {
            println!("No previous history");
        }

        loop {
            let readline = rl.readline("> ");
            match readline {
                Ok(line) => {
                    rl.add_history_entry(line.as_str());
                    rl.save_history(HISTORY_FILE).unwrap();
                    //println!("Line: {}", line);
                    let typed_command = TypedCommand::try_from(&*line)?;
                    let command = self
                        .cmd_list
                        .iter()
                        .find(|cmd| cmd.name == typed_command.name)
                        .ok_or(error::ReplError::CommandNotFoundError(typed_command.name))?;
                    (command.func)(&mut self.data, &typed_command.params)?;
                    if command.name == "quit" {
                        break;
                    } else if command.name == "help" {
                        println!("Massa client help:");
                        self.cmd_list
                            .iter()
                            .filter(|cmd| cmd.name.len() > 0)
                            .for_each(|cmd| println!(" - {}  :  {}", cmd.name, cmd.help));
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    println!("CTRL-C");
                    break;
                }
                Err(ReadlineError::Eof) => {
                    println!("CTRL-D");
                    break;
                }
                Err(err) => {
                    println!("Error: {:?}", err);
                    continue;
                }
            }
        }

        Ok(())
    }
}

fn quit_func(_data: &mut ReplData, _params: &[&str]) -> Result<(), error::ReplError> {
    println!("Buy...");
    Ok(())
}

fn help_func(_data: &mut ReplData, _params: &[&str]) -> Result<(), error::ReplError> {
    Ok(())
}

fn empty_cmd_func(_data: &mut ReplData, _params: &[&str]) -> Result<(), error::ReplError> {
    Ok(())
}
