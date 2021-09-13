// Copyright (c) 2021 MASSA LABS <info@massa.net>

#![feature(async_closure)]
#![feature(drain_filter)]
#![feature(ip)]

#[macro_use]
extern crate logging;

mod error;

pub mod network;
pub mod protocol;

pub use error::{CommunicationError, HandshakeErrorType};
pub use models::node::NodeId;
pub use network::PeerInfo;
