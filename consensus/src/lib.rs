#![feature(vecdeque_binary_search)]
#![feature(bool_to_option)]
#![feature(hash_drain_filter)]

#[macro_use]
extern crate logging;

mod block_graph;
pub mod config;
pub mod consensus_controller;
pub mod default_consensus_controller;
mod error;
mod random_selector;
mod timeslots;

#[cfg(test)]
mod tests;
