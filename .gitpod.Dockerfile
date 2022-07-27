FROM gitpod/workspace-rust

USER gitpod

RUN sudo apt-get -q update \
    && sudo apt-get install -yq libclang-dev

RUN rustup toolchain install nightly \
    && rustup default nightly \
    && rustup component add clippy
