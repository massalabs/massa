//! Copyright (c) 2022 MASSA LABS <info@massa.net>

mod block_factory;
mod endorsement_factory;
mod manager;
mod run;

pub use run::start_factory;

#[cfg(test)]
mod tests;
