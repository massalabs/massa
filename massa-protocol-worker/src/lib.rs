#![feature(map_try_insert)]
#![feature(let_chains)]

mod connectivity;
mod controller;
mod handlers;
mod manager;
mod messages;
mod sig_verifier;
mod worker;
mod wrap_network;

pub use worker::start_protocol_controller;

#[cfg(test)]
mod tests;
