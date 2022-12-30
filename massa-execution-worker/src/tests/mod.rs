// Copyright (c) 2022 MASSA LABS <info@massa.net>

#[cfg(any(test, feature = "gas_calibration", feature = "benchmarking"))]
mod mock;

#[cfg(all(not(feature = "gas_calibration"), not(feature = "benchmarking")))]
mod scenarios_mandatories;

#[cfg(all(not(feature = "gas_calibration"), not(feature = "benchmarking")))]
mod tests_active_history;

#[cfg(any(feature = "gas_calibration", feature = "benchmarking"))]
pub use mock::get_sample_state;
