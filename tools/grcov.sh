# from https://blog.balthazar-rouberol.com/measuring-the-coverage-of-a-rust-program-in-github-actions
cargo clean
export RUSTFLAGS="-Cinstrument-coverage"
cargo build
export LLVM_PROFILE_FILE="bo-%p-%m.profraw"
cargo test
grcov . -s . --binary-path ./target/debug/ -t html --branch --ignore-not-existing -o ./target/debug/coverage/
echo "grcov report written to: ./target/debug/coverage/index.html"