#![feature(destructuring_assignment)]

#[macro_use]
extern crate logging;

mod block_database;
pub mod config;
pub mod consensus_controller;
pub mod default_consensus_controller;
mod error;
mod random_selector;
mod timeslots;

#[cfg(test)]
mod tests;
