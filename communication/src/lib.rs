#![feature(ip)]
//#![feature(destructuring_assignment)]

//#[macro_use]
//extern crate log;
#[macro_use]
extern crate logging;

mod error;

pub mod network;
pub mod protocol;

pub use error::CommunicationError;
