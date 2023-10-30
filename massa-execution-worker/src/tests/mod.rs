// Copyright (c) 2022 MASSA LABS <info@massa.net>

#[cfg(any(
    test,
    feature = "gas_calibration",
    feature = "benchmarking",
    feature = "test-exports",
    test
))]
mod mock;

#[cfg(test)]
mod scenarios_mandatories;

#[cfg(test)]
mod universe;

#[cfg(test)]
mod tests_active_history;

mod interface;


#[cfg(any(
    feature = "gas_calibration",
    feature = "benchmarking",
    feature = "test-exports",
    test
))]
pub use mock::get_sample_state;
