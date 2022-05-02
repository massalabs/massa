FROM gitpod/workspace-rust

RUN bash -cl "rustup toolchain install nightly && rustup default nightly && rustup component add clippy"
