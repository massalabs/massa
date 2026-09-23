mod breaking_contracts;
mod check_gas_cost_definitions;
mod datastore_abi_callers;
mod dump_bytecode;
mod find_bytecode_string;
mod profile_address_keys;
mod scan_datastore_sizes;
mod update_package_versions;

use crate::breaking_contracts::breaking_contracts;
use crate::check_gas_cost_definitions::check_gas_cost_definitions;
use crate::datastore_abi_callers::datastore_abi_callers;
use crate::dump_bytecode::dump_bytecode;
use crate::find_bytecode_string::find_bytecode_string;
use crate::profile_address_keys::profile_address_keys;
use crate::scan_datastore_sizes::scan_datastore_sizes;
use crate::update_package_versions::update_package_versions;
use std::env;

/// to use it task: cargo xtask <task_name>
/// example: cargo xtask update_package_versions to update package versions
fn main() {
    let task = env::args().nth(1);

    match task.as_deref() {
        // We can add more tasks here
        Some("update_package_versions") => update_package_versions(),
        Some("check_gas_cost_definitions") => check_gas_cost_definitions().unwrap(),
        Some("breaking_contracts") => {
            let args: Vec<String> = env::args().skip(2).collect();
            breaking_contracts(&args).unwrap_or_else(|e| panic!("{e}"))
        }
        Some("datastore_abi_callers") => {
            let args: Vec<String> = env::args().skip(2).collect();
            datastore_abi_callers(&args).unwrap_or_else(|e| panic!("{e}"))
        }
        Some("dump_bytecode") => {
            let args: Vec<String> = env::args().skip(2).collect();
            dump_bytecode(&args).unwrap_or_else(|e| panic!("{e}"))
        }
        Some("find_bytecode_string") => {
            let args: Vec<String> = env::args().skip(2).collect();
            find_bytecode_string(&args).unwrap_or_else(|e| panic!("{e}"))
        }
        Some("profile_address_keys") => {
            let args: Vec<String> = env::args().skip(2).collect();
            profile_address_keys(&args).unwrap_or_else(|e| panic!("{e}"))
        }
        Some("scan_datastore_sizes") => {
            let args: Vec<String> = env::args().skip(2).collect();
            scan_datastore_sizes(&args).unwrap_or_else(|e| panic!("{e}"))
        }
        _ => panic!("Unknown task"),
    }
}
