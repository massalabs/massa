// Copyright (c) 2022 MASSA LABS <info@massa.net>

mod config;
mod mock;

pub use config::*;
#[cfg(feature = "testing")]
pub use mock::*;
