// Copyright (c) 2022 MASSA LABS <info@massa.net>

mod controller;
mod worker;

/// Start thread factory
pub use worker::start_factory_worker;
