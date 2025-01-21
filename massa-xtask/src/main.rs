#[cfg(feature = "gas_calibration")]
mod check_gas_cost_definitions;
mod update_package_versions;

#[cfg(feature = "gas_calibration")]
use crate::check_gas_cost_definitions::check_gas_cost_definitions;
use crate::update_package_versions::update_package_versions;
use std::env;

/// to use it task: cargo xtask <task_name>
/// example: cargo xtask update_package_versions to update package versions
fn main() {
    let task = env::args().nth(1);

    match task.as_deref() {
        // We can add more tasks here
        Some("update_package_versions") => update_package_versions(),
        #[cfg(feature = "gas_calibration")]
        Some("check_gas_cost_definitions") => check_gas_cost_definitions().unwrap(),
        _ => panic!("Unknown task"),
    }
}
