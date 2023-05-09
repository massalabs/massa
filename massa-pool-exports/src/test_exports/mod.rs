// Copyright (c) 2022 MASSA LABS <info@massa.net>

mod config;
mod mock;

pub(crate)  use config::*;
#[cfg(feature = "testing")]
pub(crate)  use mock::*;
