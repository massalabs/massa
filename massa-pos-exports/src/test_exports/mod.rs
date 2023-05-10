// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Proof of Stake module `test_exports`

mod bootstrap;
mod mock;

pub use bootstrap::assert_eq_pos_state;
#[cfg(feature = "testing")]
pub(crate) use mock::*;
