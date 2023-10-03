# https://github.com/taiki-e/cargo-llvm-cov#installation
cargo llvm-cov clean --workspace # remove artifacts that may affect the coverage results
cargo llvm-cov test --open --workspace --features testing --ignore-filename-regex "test_exports|test_helpers|massa-xtask"
