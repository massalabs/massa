//! Copyright (c) 2022 MASSA LABS <info@massa.net>

#![feature(deadline_api)]

mod block_factory;
mod endorsement_factory;
mod manager;
mod run;

pub(crate)  use run::start_factory;

#[cfg(test)]
mod tests;
