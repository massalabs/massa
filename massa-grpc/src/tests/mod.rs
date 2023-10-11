// Copyright (c) 2023 MASSA LABS <info@massa.net>
/// mock for testing
#[cfg(any(test, feature = "testing"))]
pub mod mock;

#[cfg(test)]
mod public;
#[cfg(test)]
mod stream;
