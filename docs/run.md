# Running a node

## From binaries

Simply run the binaries you downloaded in the previous step:
Open the `massa-node` folder and run the `massa-node` excutable
Open the `massa-client` folder and run the `massa-client` excutable

## From source code

### On Ubuntu / MacOS

#### Start the node

On a first window:

```bash
cd massa/massa-node/
```

Launch the node, on Ubuntu:

```bash
RUST_BACKTRACE=full cargo run --release |& tee logs.txt
```

**Or,** on macOS:

```bash
RUST_BACKTRACE=full cargo run --release > logs.txt 2>&1
```

You should leave the window opened.

#### Start the client

On a second window:

```bash
cd massa/massa-client/
```

Then:

```bash
cargo run --release
```

Please wait until the directories are built before moving to the next step.

### On Windows

#### Start the Node

-   Open Windows Power Shell or Command Prompt on a first window
    -   Type: `cd massa`
    -   Type: `cd massa-node`
    -   Type: `cargo run --release`

You should leave the window opened.

#### Start the Client

-   Open Windows Power Shell or Command Prompt on a second window
    -   Type: `cd massa`
    -   Type: `cd massa-client`
    -   Type: `cargo run --release`

Please wait until the directories are built before moving to the next step.

## Next step

-   [Creating a wallet](https://github.com/massalabs/massa/wiki/wallet)
