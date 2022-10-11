// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Definition and exports of the PoS types and errors.
//!
//! Define also the Selector worker that compute in background the draws for
//! the future cycles

#![warn(missing_docs)]
#![feature(map_first_last)]
#![feature(let_chains)]

mod controller_traits;
mod cycle_info;
mod deferred_credits;
mod error;
mod pos_final_state_impl;
mod settings;
mod types;

pub use controller_traits::{SelectorController, SelectorManager};
pub use cycle_info::*;
pub use deferred_credits::*;
pub use error::*;
pub use settings::SelectorConfig;
pub use types::*;

#[cfg(feature = "testing")]
pub mod test_exports;
