// Copyright (c) 2021 MASSA LABS <info@massa.net>

#![feature(drain_filter)]
#![feature(ip)]
#![feature(async_closure)]

#[macro_use]
extern crate logging;

mod common;
mod error;

pub mod network;
pub mod protocol;

pub use common::NodeId;
pub use error::{CommunicationError, HandshakeErrorType};
pub use network::PeerInfo;
