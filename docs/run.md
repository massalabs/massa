# Running a node

## On Ubuntu / MacOS

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


## On Windows

### Start the Node

-   Open Windows Power Shell or Command Prompt on a first window
    -   Type: `cd massa`
    -   Type: `cd massa-node`
    -   Type: `cargo run --release`

Your node is running. Once the process is finished, you should leave the
window opened.

You can now launch your client and move toward the wallet creation then
the staking process.

### Start the Client

-   Open Windows Power Shell or Command Prompt on a second window
    -   Type: `cd massa`
    -   Type: `cd massa-client`
    -   Type: `cargo run --release`

## Next step

-   [Creating a wallet](docs/wallet.md)