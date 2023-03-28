#[cfg(feature = "benchmarking")]
use criterion::{black_box, criterion_group, criterion_main, Criterion};

#[cfg(feature = "benchmarking")]
fn criterion_benchmark(c: &mut Criterion) {
    use massa_execution_worker::InterfaceImpl;
    use massa_models::address::Address;
    use massa_sc_runtime::{run_main, GasCosts};
    use rand::Rng;
    use std::path::PathBuf;
    use std::str::FromStr;

    /// This function is used to prepare the data for the benchmarks
    /// It prepare the interface and the contracts to be executed.
    fn prepare_bench_function() -> (InterfaceImpl, Vec<Vec<u8>>, GasCosts) {
        let interface = InterfaceImpl::new_default(
            black_box(
                Address::from_str("AU12cMW9zRKFDS43Z2W88VCmdQFxmHjAo54XvuVV34UzJeXRLXW9M").unwrap(),
            ),
            black_box(None),
        );
        let gas_costs = GasCosts::new(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../massa-node/base_config/gas_costs/abi_gas_costs.json"
            )
            .into(),
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../massa-node/base_config/gas_costs/wasm_gas_costs.json"
            )
            .into(),
        )
        .unwrap();
        let base_path = concat!(env!("CARGO_MANIFEST_DIR"), "/benches/wasm");
        let contracts_names = vec!["prints.wasm", "event_callstack.wasm"];
        let contracts = contracts_names
            .iter()
            .map(|name| std::fs::read::<PathBuf>(format!("{}/{}", base_path, name).into()).unwrap())
            .collect::<Vec<Vec<u8>>>();
        (interface, contracts, gas_costs)
    }

    c.bench_function("Same execution", |b| {
        let (interface, contracts, gas_costs) = prepare_bench_function();
        b.iter(|| {
            let contract_id = 0;
            run_main(
                contracts.get(contract_id).unwrap(),
                2_000_000_000,
                &interface,
                gas_costs.clone(),
            )
            .unwrap()
        })
    });

    c.bench_function("2 different executions", |b| {
        let mut rng = rand::thread_rng();
        let (interface, contracts, gas_costs) = prepare_bench_function();
        b.iter(|| {
            let contract_id = rng.gen_range(0..2);
            run_main(
                contracts.get(contract_id).unwrap(),
                2_000_000_000,
                &interface,
                gas_costs.clone(),
            )
            .unwrap()
        })
    });
}

#[cfg(feature = "benchmarking")]
criterion_group!(benches, criterion_benchmark);

#[cfg(feature = "benchmarking")]
criterion_main!(benches);

#[cfg(not(feature = "benchmarking"))]
fn main() {
    println!("Please use the `--features benchmarking` flag to run this benchmark.");
}
