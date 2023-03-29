// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Definition and exports of the PoS types and errors.
//!
//! Define also the Selector worker that compute in background the draws for
//! the future cycles

#![warn(missing_docs)]
#![feature(let_chains)]

mod config;
mod controller_traits;
mod cycle_info;
mod deferred_credits;
mod error;
mod pos_changes;
mod pos_final_state;
mod settings;

pub use config::PoSConfig;
pub use controller_traits::{Selection, SelectorController, SelectorManager};
pub use cycle_info::*;
pub use deferred_credits::*;
pub use error::*;
pub use pos_changes::*;
pub use pos_final_state::*;
pub use settings::SelectorConfig;

#[cfg(feature = "testing")]
pub mod test_exports;
