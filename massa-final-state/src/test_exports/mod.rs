//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file exports testing utilities

mod bootstrap;
mod config;

pub use bootstrap::{assert_eq_final_state, assert_eq_final_state_hash, create_final_state};
pub(crate) use config::*;
