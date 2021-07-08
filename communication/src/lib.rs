#![feature(ip)]

#[macro_use]
extern crate logging;

mod common;
mod error;

pub mod network;
pub mod protocol;

pub use common::NodeId;
pub use error::{CommunicationError, HandshakeErrorType};
pub use network::PeerInfo;
