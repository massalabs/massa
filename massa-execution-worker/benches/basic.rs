#[cfg(feature = "benchmarking")]
use criterion::{black_box, criterion_group, criterion_main, Criterion};

#[cfg(feature = "benchmarking")]
fn criterion_benchmark(c: &mut Criterion) {
    use massa_execution_worker::InterfaceImpl;
    use massa_models::address::Address;
    use massa_sc_runtime::{run_main, GasCosts};
    use std::path::PathBuf;
    use std::str::FromStr;

    let interface = InterfaceImpl::new_default(
        black_box(
            Address::from_str("A12cMW9zRKFDS43Z2W88VCmdQFxmHjAo54XvuVV34UzJeXRLXW9M").unwrap(),
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
    c.bench_function("Basic execution", |b| {
        b.iter(|| {
            run_main(
                &std::fs::read::<PathBuf>(
                    concat!(env!("CARGO_MANIFEST_DIR"), "/benches/test.wasm").into(),
                )
                .unwrap(),
                2_000_000,
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
