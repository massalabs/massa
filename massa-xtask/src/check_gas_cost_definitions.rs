use massa_sc_runtime::GasCosts;
use std::collections::HashMap;
use std::path::PathBuf;

const ABI_GAS_COSTS_FILE: &str = "massa-node/base_config/gas_costs/abi_gas_costs.json";

pub(crate) fn check_gas_cost_definitions() -> Result<(), String> {
    // Check gas cost definitions between:
    // massa-node/base_config/gas_costs/abi_gas_costs.json
    // massa-sc-runtime GasCosts

    // `GasCosts::new` fails if any cost required by the runtime is missing
    // from the file, so this catches definitions the node needs but the
    // shipped file lacks (the node would not start).
    GasCosts::new(PathBuf::from(ABI_GAS_COSTS_FILE))
        .map_err(|err| format!("failed to load {}: {}", ABI_GAS_COSTS_FILE, err))?;

    // Detect stale definitions: a key that can be removed without making
    // `GasCosts::new` fail is no longer used by the runtime. The runtime
    // does not expose its expected key set, so probe by removing keys one
    // at a time instead of maintaining a duplicate list here. Stale keys
    // are harmless at runtime, so only warn about them.
    let file_content = std::fs::read_to_string(ABI_GAS_COSTS_FILE)
        .map_err(|err| format!("failed to read {}: {}", ABI_GAS_COSTS_FILE, err))?;
    let costs: HashMap<String, u64> = serde_json::from_str(&file_content)
        .map_err(|err| format!("failed to parse {}: {}", ABI_GAS_COSTS_FILE, err))?;

    let probe_file = std::env::temp_dir().join("massa_xtask_abi_gas_costs_probe.json");
    for key in costs.keys() {
        let mut probe_costs = costs.clone();
        probe_costs.remove(key);
        let probe_content = serde_json::to_string(&probe_costs).map_err(|err| err.to_string())?;
        std::fs::write(&probe_file, probe_content).map_err(|err| err.to_string())?;
        if GasCosts::new(probe_file.clone()).is_ok() {
            println!("Found in json but not used by the runtime: {key}");
        }
    }
    let _ = std::fs::remove_file(&probe_file);

    println!("Gas cost definitions OK");
    Ok(())
}
