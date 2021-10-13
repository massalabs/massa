// Copyright (c) 2021 MASSA LABS <info@massa.net>

//!Rustyline integration to manage command completion.
//!
//! See [Rustyline documentation](https://docs.rs/rustyline) for trait impl

use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::Direction;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::Helper;
use rustyline::{Context, Result};

pub const HISTORY_FILE: &str = "config/history.txt";

///the main struct that implements Rustyline traits.
pub struct ReplHelper {
    completer: CommandCompleter,
    hinter: CommandHinter,
    validator: CommandValidator,
}

impl ReplHelper {
    /// Creates a new ReplHelper
    pub fn new() -> ReplHelper {
        ReplHelper {
            completer: CommandCompleter::new(),
            hinter: CommandHinter::new(),
            validator: CommandValidator::new(),
        }
    }

    /// adds given command name to ReplHelper
    pub fn add_cmd(&mut self, cmd_name: String, min_nb_param: usize, max_nb_param: usize) {
        self.completer.cmd_list.push(cmd_name.clone());
        self.hinter.cmd_list.push(cmd_name.clone());
        self.validator.cmd_list.push(HelperCommand {
            name: cmd_name,
            max_nb_param,
            min_nb_param,
        });
    }
}

#[derive(Debug)]
struct Entry {
    index: Option<usize>,
    command: String,
}

fn recent_commands(line: &str, ctx: &Context, cmd_list: &[String]) -> Vec<Entry> {
    let mut entries = vec![];

    let start = ctx.history_index().saturating_sub(1);

    for command in cmd_list {
        if !line.is_empty() && command.starts_with(line) {
            let index = ctx
                .history()
                .starts_with(command, start, Direction::Reverse);

            let entry = Entry {
                index,
                command: command.to_string(),
            };
            entries.push(entry);
        }
    }

    // Find the most recent command.
    entries.sort_by(|x, y| y.index.cmp(&x.index));

    entries
}

struct HelperCommand {
    name: String,
    max_nb_param: usize,
    min_nb_param: usize,
}

#[derive(Default)]
struct CommandCompleter {
    cmd_list: Vec<String>,
}

impl CommandCompleter {
    fn new() -> Self {
        Self::default()
    }

    fn complete(&self, line: &str, _pos: usize, ctx: &Context) -> Result<(usize, Vec<Pair>)> {
        let entries = recent_commands(line, ctx, &self.cmd_list);

        let mut pairs = vec![];

        for entry in entries {
            let pair = Pair {
                display: entry.command.to_string(),
                replacement: entry.command.to_string(),
            };

            pairs.push(pair);
        }

        Ok((0, pairs))
    }
}

#[derive(Default)]
struct CommandHinter {
    cmd_list: Vec<String>,
}

impl CommandHinter {
    fn new() -> Self {
        Self::default()
    }

    fn hint(&self, line: &str, pos: usize, ctx: &Context) -> Option<String> {
        let entries = recent_commands(line, ctx, &self.cmd_list);

        if entries.is_empty() {
            None
        } else {
            Some(entries[0].command[pos..].to_string())
        }
    }
}

#[derive(Default)]
struct CommandValidator {
    cmd_list: Vec<HelperCommand>,
}

impl CommandValidator {
    fn new() -> Self {
        Self::default()
    }

    fn validate(&self, ctx: &mut ValidationContext) -> Result<ValidationResult> {
        let input = ctx.input();

        let mut result = ValidationResult::Invalid(Some("\nInvalid input".to_string()));

        if input.is_empty() {
            result = ValidationResult::Valid(None);
        } else {
            for command in &self.cmd_list {
                let params: Vec<&str> = input.split(' ').collect();
                if command.name.starts_with(&params[0]) {
                    // validate parameters
                    /*                    println!(
                        " params.len():{} command.max_nb_param:{}",
                        params.len(),
                        command.max_nb_param
                    );*/
                    if params.len() > command.min_nb_param
                        && params.len() <= command.max_nb_param + 1
                    {
                        result = ValidationResult::Valid(None);
                    } else {
                        result =
                            ValidationResult::Invalid(Some("\nInvalid parameters".to_string()));
                    }
                    break;
                }
            }
        }

        Ok(result)
    }

    // XXX: Doesn't seem to make a difference.
    // fn validate_while_typing(&self) -> bool {
    //     true
    // }
}

impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(&self, line: &str, pos: usize, ctx: &Context) -> Result<(usize, Vec<Pair>)> {
        self.completer.complete(line, pos, ctx)
    }
}

impl Hinter for ReplHelper {
    type Hint = String;
    fn hint(&self, line: &str, pos: usize, ctx: &Context) -> Option<String> {
        self.hinter.hint(line, pos, ctx)
    }
}

impl Validator for ReplHelper {
    fn validate(&self, ctx: &mut ValidationContext) -> Result<ValidationResult> {
        self.validator.validate(ctx)
    }

    // fn validate_while_typing(&self) -> bool {
    //     self.validator.validate_while_typing()
    // }
}

impl Highlighter for ReplHelper {}

impl Helper for ReplHelper {}
