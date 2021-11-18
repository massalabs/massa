# Massa: The Decentralized and Scaled Blockchain

Massa is a truly decentralized blockchain controlled by thousands of
people. With the breakthrough multithreaded technology, we're set for
mass adoption.

[![CI](https://github.com/massalabs/massa/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/massalabs/massa/actions/workflows/ci.yml?query=branch%3Amain)
[![Coverage Status](https://coveralls.io/repos/github/massalabs/massa/badge.svg?branch=main)](https://coveralls.io/github/massalabs/massa?branch=main)

## Introduction

[Massa](https://massa.net) is a new blockchain reaching a high
transaction throughput in a decentralized network. Our research is
published in this [technical paper](https://arxiv.org/pdf/1803.09029).
It shows that throughput of 10'000 transactions per second is reached
even in a fully decentralized network with thousands of nodes.

An easy-to-read blog post introduction with videos is written
[here](https://massa.net/blog/post/0/).

We are now releasing the **Massa testnet** in this Gitlab repository,
with its explorer available at <https://test.massa.net>.

## Testnet Incentives

As decentralization is our core value, we would like to help you start
and run a Massa node. Running a node during the testnet phase also helps
us find bugs and improve usability, so it will be rewarded with real
Massa on mainnet launch.

The mechanics of those rewards are described in the [Testnet
rules](docs/testnet_rules.md).

## Testnet Discussions

Please come to our [Discord](https://discord.com/invite/TnsJQzXkRN) for
testnet discussions, in the testnet channel.

For project announcements, we mainly use
[Telegram](https://t.me/massanetwork).

## Install

If you just wish to run a Massa node without compiling it yourself, you
can run the latest binary:

-   [Windows
    executable](https://gitlab.com/massalabs/massa/-/jobs/artifacts/testnet/download?job=build-windows)
-   [Linux
    binary](https://gitlab.com/massalabs/massa/-/jobs/artifacts/testnet/download?job=build-linux)
-   [MacOS
    binary](https://gitlab.com/massalabs/massa/-/jobs/artifacts/testnet/download?job=build-darwin)

### On Windows

Please go to the [Install and Run on Windows](docs/windows_install.md)
page.

### On Ubuntu / MacOS

-   on Ubuntu, these libs must be installed:
    `sudo apt install pkg-config curl git build-essential libssl-dev`
-   install [rustup](https://www.rust-lang.org/tools/install):
    `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
-   configure path: `source $HOME/.cargo/env`
-   check rust version: `rustc --version`
-   install
    [nigthly](https://doc.rust-lang.org/edition-guide/rust-2018/rustup-for-managing-rust-versions.html):
    `rustup toolchain install nightly`
-   set it as default: `rustup default nightly`
-   check rust version: `rustc --version`
-   clone this repo:
    `git clone --branch testnet https://gitlab.com/massalabs/massa.git`

## Run

### Start the node

On a first window:

    cd massa/massa-node/

Launch the node, on Ubuntu:

    RUST_BACKTRACE=full cargo run --release |& tee logs.txt

On macOS:

    RUST_BACKTRACE=full cargo run --release > logs.txt 2>&1

### Start the client

On a second window:

    cd massa/massa-client/
    cargo run --release

## Update

If you use the binaries, simply download the latest binaries. Otherwise:

Update Rust:

    rustup update

Update Massa:

    cd massa/
    git stash
    git checkout testnet
    git pull

test

## Tutorials

Here is a set of tutorials:

-   [Creating a wallet](docs/wallet.md)
-   [Staking](docs/staking.md)
-   [Sending transactions](docs/transaction.md)
-   [Install and Run on Windows](docs/windows_install.md)
-   [Routability tutorial](docs/routability.md)
-   [Testnet rewards program](docs/testnet_rules.md)
-   To get testnet coins, send your address to the faucet bot in the
    "testnet-faucet" channel of our
    [Discord](https://discord.com/invite/TnsJQzXkRN).

## FAQ and Troubleshooting

You'll find answers to common issues and questions regarding the Massa
protocol in the [FAQ](docs/faq.md).

Don't hesitate to ask questions in the
[Discord](https://discord.com/invite/TnsJQzXkRN) testnet channel.
