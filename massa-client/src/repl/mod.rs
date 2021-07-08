use crate::repl::helper::HISTORY_FILE;
use core::convert::TryFrom;
use rustyline::error::ReadlineError;
use rustyline::Helper;
use rustyline::{CompletionType, Config, Editor};
use std::net::SocketAddr;

pub mod error;
mod helper;

pub struct ReplData {
    pub node_ip: SocketAddr,
    pub cli: bool,
}
impl Default for ReplData {
    fn default() -> Self {
        ReplData {
            node_ip: "0.0.0.0:3030".parse().unwrap(),
            cli: false,
        }
    }
}

pub type CmdFn = Box<dyn Fn(&mut ReplData, &[&str]) -> Result<(), error::ReplError> + Send + Sync>;

pub struct Command {
    name: String,
    max_nb_param: usize,
    min_nb_param: usize,
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
    //set to true if the command is executed as a command line.
}

impl Repl {
    pub fn new() -> Self {
        let mut repl = Repl {
            data: Default::default(),
            cmd_list: Vec::new(),
        };

        repl.cmd_list.push(Command {
            name: "quit".to_string(),
            max_nb_param: 0,
            min_nb_param: 0,
            help: "quit Massa client".to_string(),
            func: Box::new(quit_func),
        });
        repl.cmd_list.push(Command {
            name: "help".to_string(),
            max_nb_param: 0,
            min_nb_param: 0,
            help: "this help".to_string(),
            func: Box::new(help_func),
        });
        repl.cmd_list.push(Command {
            name: "".to_string(),
            max_nb_param: 0,
            min_nb_param: 0,
            help: "".to_string(),
            func: Box::new(empty_cmd_func),
        });

        repl
    }

    pub fn new_command_noargs<'a, 'b, S, F>(
        &mut self,
        name: S,
        help: S,
        func: F,
        app: clap::App<'a, 'b>,
    ) -> clap::App<'a, 'b>
    where
        S: ToString,
        F: Fn(&mut ReplData, &[&str]) -> Result<(), error::ReplError> + Send + Sync + 'static,
    {
        self.new_command(name, help, 0, 0, func, app)
    }

    //max_nb_param is the max number of parameters that the command can take.
    pub fn new_command<'a, 'b, S, F>(
        &mut self,
        name: S,
        help: S,
        min_nb_param: usize,
        max_nb_param: usize,
        func: F,
        app: clap::App<'a, 'b>,
    ) -> clap::App<'a, 'b>
    where
        S: ToString,
        F: Fn(&mut ReplData, &[&str]) -> Result<(), error::ReplError> + Send + Sync + 'static,
    {
        self.cmd_list.push(Command {
            name: name.to_string(),
            max_nb_param,
            min_nb_param,
            help: help.to_string(),
            func: Box::new(func),
        });

        if min_nb_param > 0 {
            app.subcommand(
                clap::SubCommand::with_name(&name.to_string())
                    .about("")
                    .arg(
                        clap::Arg::with_name("")
                            .required(true)
                            .min_values(min_nb_param as u64)
                            .max_values(max_nb_param as u64),
                    ),
            )
        } else {
            app.subcommand(
                clap::SubCommand::with_name(&name.to_string())
                    .about("")
                    .arg(clap::Arg::with_name("").required(false)),
            )
        }
    }

    pub fn run(mut self) {
        let mut helper = helper::ReplHelper::new();
        let config = Config::builder()
            //.completion_type(CompletionType::Circular)
            .completion_type(CompletionType::List)
            .build();

        //declare all cmd
        self.cmd_list
            .iter()
            .for_each(|cmd| helper.add_cmd(cmd.name.clone(), cmd.min_nb_param, cmd.max_nb_param));

        let mut rl = Editor::with_config(config);
        rl.set_helper(Some(helper));

        if rl.load_history(HISTORY_FILE).is_err() {
            println!("No previous history");
        }

        loop {
            let readline = rl.readline("> ");
            match readline {
                Ok(line) => match self.readline(line, &mut rl) {
                    Ok(quit) => {
                        if quit {
                            break;
                        }
                    }
                    Err(error::ReplError::CommandNotFoundError(name)) => {
                        println!("Command:{} not found.", name)
                    }
                    Err(err) => println!("Cmd exec error:{}", err),
                },
                Err(ReadlineError::Interrupted) => {
                    println!("CTRL-C");
                    break;
                }
                Err(ReadlineError::Eof) => {
                    println!("CTRL-D");
                    break;
                }
                Err(err) => {
                    println!("Read input error: {:?}", err);
                    continue;
                }
            }
        }
    }

    pub fn run_cmd(&mut self, cmd: &str, args: &[&str]) {
        let mut helper = helper::ReplHelper::new();
        let config = Config::builder()
            //.completion_type(CompletionType::Circular)
            .completion_type(CompletionType::List)
            .build();
        //declare all cmd
        self.cmd_list
            .iter()
            .for_each(|cmd| helper.add_cmd(cmd.name.clone(), cmd.min_nb_param, cmd.max_nb_param));

        let mut rl = Editor::<helper::ReplHelper>::with_config(config);
        let line = format!(
            "{}{}",
            cmd,
            args.iter()
                .fold(String::new(), |res, arg| format!("{} {}", res, arg))
        );
        if let Err(err) = self.readline(line, &mut rl) {
            match err {
                error::ReplError::CommandNotFoundError(name) => {
                    println!("Command:{} not found.", name)
                }
                _ => println!("Cmd exec error:{}", err),
            }
        }
    }

    fn readline<H: Helper>(
        &mut self,
        line: String,
        rl: &mut Editor<H>,
    ) -> Result<bool, error::ReplError> {
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
            return Ok(true);
        } else if command.name == "help" {
            println!("Massa client help:");
            self.cmd_list
                .iter()
                .filter(|cmd| cmd.name.len() > 0)
                .for_each(|cmd| println!(" - {}  :  {}", cmd.name, cmd.help));
        }
        Ok(false)
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
