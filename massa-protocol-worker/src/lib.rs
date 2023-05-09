#![feature(map_try_insert)]
#![feature(let_chains)]
#![feature(ip)]

mod connectivity;
mod controller;
mod handlers;
mod manager;
mod messages;
mod sig_verifier;
mod worker;
mod wrap_network;

pub(crate)  use worker::{create_protocol_controller, start_protocol_controller};

#[cfg(test)]
mod tests;
