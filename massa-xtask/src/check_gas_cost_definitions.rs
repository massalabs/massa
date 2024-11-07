use massa_sc_runtime::GasCosts;
use std::collections::HashSet;

pub(crate) fn check_gas_cost_definitions() -> Result<(), String> {
    // Check gas cost definition between:
    // massa-node/base_config/gas_costs/abi_gas_costs.json
    // massa-sc-runtime GasCosts::default()

    let gas_costs = GasCosts::default();
    let gas_costs_abi_defined = gas_costs
        .get_abi_costs()
        .keys()
        .cloned()
        .collect::<HashSet<String>>();

    let massa_node_gas_costs = GasCosts::new(
        // SETTINGS.execution.abi_gas_costs_file.clone(),
        // SETTINGS.execution.wasm_gas_costs_file.clone(),
        "massa-node/base_config/gas_costs/abi_gas_costs.json".into(),
    )
    .expect("Failed to load gas costs");

    let massa_node_gas_costs_abi_defined = massa_node_gas_costs
        .get_abi_costs()
        .keys()
        .cloned()
        .collect::<HashSet<String>>();

    let mut found_diff = false;
    let diff_1 = gas_costs_abi_defined.difference(&massa_node_gas_costs_abi_defined);
    for x1 in diff_1 {
        println!("Found in default() but not in json: {x1}");
        found_diff = true;
    }

    let diff_2 = massa_node_gas_costs_abi_defined.difference(&gas_costs_abi_defined);
    let exclude_list = HashSet::from([
        "cl_compilation",
        "launch",
        "sp_compilation",
        "launch_wasmv1",
        "max_instance",
    ]);
    for x2 in diff_2 {
        if !exclude_list.contains(x2.as_str()) {
            println!("Found in json but not in default(): {x2}");
        }
    }

    if found_diff {
        Err("Found gas costs definition differences".to_string())
    } else {
        Ok(())
    }
}
