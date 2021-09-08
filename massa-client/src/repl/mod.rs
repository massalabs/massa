// Copyright (c) 2021 MASSA LABS <info@massa.net>

//! Rustyline integration to manager user input and command typing and execution.
//!
//! Command are registered with new_command_noargs or new_command repl functions.
//! repl API register all elements in Rustyline and Claps.

use crate::repl::helper::HISTORY_FILE;
use core::convert::TryFrom;
use rustyline::error::ReadlineError;
use rustyline::Helper;
use rustyline::{CompletionType, Config, Editor};
use wallet::ReplData;

mod helper;

pub type CmdFn =
    Box<dyn Fn(&mut ReplData, &[&str]) -> Result<(), models::error::ReplError> + Send + Sync>;

///Define a command that can be executed.
///
/// name: typed name of the command
///
/// min_nb_param, max_nb_param: min and max number of parameters of the command
///
/// help: display help
///
/// func: executed function when the command is typed.
pub struct Command {
    name: String,
    max_nb_param: usize,
    min_nb_param: usize,
    help: String,
    active: bool,
    func: CmdFn,
}

//A command name with its parameters that can be executed by the client.
struct TypedCommand<'a> {
    name: String,
    params: Vec<&'a str>,
}

impl<'a> TryFrom<&'a str> for TypedCommand<'a> {
    type Error = models::error::ReplError;

    fn try_from(line: &'a str) -> Result<Self, Self::Error> {
        let mut iter = line.split(' ');
        let name = iter
            .next()
            .ok_or(models::error::ReplError::ParseCommandError)?
            .to_string();
        let params: Vec<&str> = iter.collect();
        Ok(TypedCommand { name, params })
    }
}

/// The builder part of Repl to register the command with clap and Rustyline.
/// Use the builder pattern.
pub struct BuilderRepl<'a, 'b> {
    repl: Repl,
    app: clap::App<'a, 'b>,
}

impl<'a, 'b> BuilderRepl<'a, 'b> {
    fn new(repl: Repl, app: clap::App<'a, 'b>) -> Self {
        BuilderRepl { repl, app }
    }

    pub fn split(self) -> (Repl, clap::App<'a, 'b>) {
        (self.repl, self.app)
    }

    pub fn new_command_noargs<S, F>(self, name: S, help: S, active: bool, func: F) -> Self
    where
        S: ToString,
        F: Fn(&mut ReplData, &[&str]) -> Result<(), models::error::ReplError>
            + Send
            + Sync
            + 'static,
    {
        self.new_command(name, help, 0, 0, active, func)
    }

    pub fn new_command<S, F>(
        mut self,
        name: S,
        help: S,
        min_nb_param: usize,
        max_nb_param: usize,
        active: bool,
        func: F,
    ) -> Self
    where
        S: ToString,
        F: Fn(&mut ReplData, &[&str]) -> Result<(), models::error::ReplError>
            + Send
            + Sync
            + 'static,
    {
        self.repl.cmd_list.push(Command {
            name: name.to_string(),
            max_nb_param,
            min_nb_param,
            help: help.to_string(),
            active,
            func: Box::new(func),
        });

        self.app = if min_nb_param > 0 {
            self.app.subcommand(
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
            self.app.subcommand(
                clap::SubCommand::with_name(&name.to_string())
                    .about("")
                    .arg(clap::Arg::with_name("").required(false)),
            )
        };

        self
    }
}

/// Manage the REPL mode and typed command
/// Main struct to manager user's typed command.
///
/// Command are registered using a builder pattern with the new_command_noargs or new_command function.
/// The BuilderRepl struct implement the builder pattern and the slip function is call
/// after registering all the command to get Repl and Clap structures.
///
/// Default command are automatically created: quit, help, empty for entry cmd.
///
/// # Example
///
///
/// ```
///     let (mut repl, app) = repl::Repl::new().new_command(
///        "cmd1",
///        "help of cmd1",
///        1,
///        2,
///        cmd1_call_function,
///        app //Clap structure
///    )
///    .new_command_noargs("cmd2", "help cmd2", cmd2_call_function)
///    .split();
/// ```
/// function example:
/// ```
/// fn cmd1_call_function(_: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
///    Ok(())
/// }
/// ```
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

        repl.cmd_list.push(Command {
            name: "quit".to_string(),
            max_nb_param: 0,
            min_nb_param: 0,
            help: "quit Massa client".to_string(),
            active: true,
            func: Box::new(quit_func),
        });
        repl.cmd_list.push(Command {
            name: "help".to_string(),
            max_nb_param: 1,
            min_nb_param: 0,
            help: "this help".to_string(),
            active: true,
            func: Box::new(help_func),
        });
        repl.cmd_list.push(Command {
            name: "".to_string(),
            max_nb_param: 0,
            min_nb_param: 0,
            help: "".to_string(),
            active: true,
            func: Box::new(empty_cmd_func),
        });

        repl
    }

    /* not use in current massa_client
    ///create a new command with no args.
     pub fn new_command_noargs<'a, 'b, S, F>(
         self,
         name: S,
         help: S,
         func: F,
         active: bool,
         app: clap::App<'a, 'b>,
     ) -> BuilderRepl<'a, 'b>
     where
         S: ToString,
         F: Fn(&mut ReplData, &[&str]) -> Result<(), error::ReplError> + Send + Sync + 'static,
     {
         BuilderRepl::new(self, app).new_command(name, help, 0, 0, func)
     }
    name: name of the command. It's the data that are typed to execute a cmd.
    help: help message shown by the help command.
    func: function executed when the cmd is typed.
    app: present in the first cmd declaration call to start the builder pattern.

    active: determine if the cmd is active or not. Non active cmd can be activated later with the activate_command function.

    Wallet command for example can only use when a wallet file is defined in the client start parameters.
    By default wallet cmd are non active (false) and activated when the client parameters are processed.

    There's a difficulty with Clap. It parse client parameters and the executed command. All these the data are only available at the end.
    So its impossible to know it the wallet cmd for example are active when the cmd are declared. To wallet cmd are declared inactive
    and when the wallet file parameter is process they are activated if it's present.
     */

    ///create a new command with min and max args.
    pub fn new_command<'a, 'b, S, F>(
        self,
        name: S,
        help: S,
        min_nb_param: usize,
        max_nb_param: usize,
        func: F,
        active: bool,
        app: clap::App<'a, 'b>,
    ) -> BuilderRepl<'a, 'b>
    where
        S: ToString,
        F: Fn(&mut ReplData, &[&str]) -> Result<(), models::error::ReplError>
            + Send
            + Sync
            + 'static,
    {
        BuilderRepl::new(self, app).new_command(
            name,
            help,
            min_nb_param,
            max_nb_param,
            active,
            func,
        )
    }

    ///active the command with specified name.
    pub fn activate_command(&mut self, name: &str) {
        if let Some(cmd) = self
            .cmd_list
            .iter_mut()
            .filter(|cmd| !cmd.active)
            .find(|cmd| cmd.name == name)
        {
            cmd.active = true
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
            .filter(|cmd| cmd.active)
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
                    Err(models::error::ReplError::CommandNotFoundError(name)) => {
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

    //execute a cmd in cli mode.
    pub fn run_cmd(&mut self, cmd: &str, args: &[&str]) {
        let mut helper = helper::ReplHelper::new();
        let config = Config::builder()
            //.completion_type(CompletionType::Circular)
            .completion_type(CompletionType::List)
            .build();
        //declare all cmd
        self.cmd_list
            .iter()
            .filter(|cmd| cmd.active)
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
                models::error::ReplError::CommandNotFoundError(name) => {
                    println!("Command:{} not found.", name)
                }
                _ => println!("Cmd exec error:{}", err),
            }
        }
    }

    //execute a command using a taped line or client parameters.
    fn readline<H: Helper>(
        &mut self,
        line: String,
        rl: &mut Editor<H>,
    ) -> Result<bool, models::error::ReplError> {
        rl.add_history_entry(line.as_str());
        rl.save_history(HISTORY_FILE).unwrap();
        //println!("Line: {}", line);
        let typed_command = TypedCommand::try_from(&*line)?;
        let command = self
            .cmd_list
            .iter()
            .find(|cmd| cmd.name == typed_command.name)
            .ok_or(models::error::ReplError::CommandNotFoundError(
                typed_command.name.clone(),
            ))?;
        (command.func)(&mut self.data, &typed_command.params)?;
        if command.name == "quit" {
            return Ok(true);
        } else if command.name == "help" {
            if typed_command.params.len() > 0 {
                let command = self
                    .cmd_list
                    .iter()
                    .find(|cmd| cmd.name == typed_command.params[0])
                    .ok_or(models::error::ReplError::CommandNotFoundError(
                        typed_command.params[0].to_string(),
                    ))?;
                println!(" - {}  :  {}", command.name, command.help);
            } else {
                println!("Massa client help:");
                self.cmd_list
                    .iter()
                    .filter(|cmd| cmd.active && !cmd.name.is_empty())
                    .for_each(|cmd| println!(" - {}  :  {}", cmd.name, cmd.help));
            }
        }
        Ok(false)
    }
}

fn quit_func(_data: &mut ReplData, _params: &[&str]) -> Result<(), models::error::ReplError> {
    println!("Bye...");
    Ok(())
}

fn help_func(_data: &mut ReplData, _params: &[&str]) -> Result<(), models::error::ReplError> {
    Ok(())
}

fn empty_cmd_func(_data: &mut ReplData, _params: &[&str]) -> Result<(), models::error::ReplError> {
    Ok(())
}
