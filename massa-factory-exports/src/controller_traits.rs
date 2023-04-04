//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module exports generic traits representing interfaces for interacting
//! with the factory worker.

/// Factory manager used to stop the factory thread
pub trait FactoryManager {
    /// Stop the factory thread
    /// Note that we do not take self by value to consume it
    /// because it is not allowed to move out of `Box<dyn FactoryManager>`
    /// This will improve if the `unsized_fn_params` feature stabilizes enough to be safely usable.
    fn stop(&mut self);
}
