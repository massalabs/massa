FROM gitpod/workspace-rust

USER gitpod

RUN sudo apt-get -q update \
    && sudo apt-get install -yq libclang-dev

RUN rustup toolchain install stable \
    && rustup default stable \
    && rustup component add clippy
