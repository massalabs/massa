# Install and Run on Windows

## Set up your Rust environment

-   On Windows, you should first follow the indications from Microsoft
    to be able to run on a Rust environment
    [here](https://docs.microsoft.com/en-gb/windows/dev-environment/rust/setup).
    -   Install Visual Studio (recommended) or the Microsoft C++ Build
        Tools
    -   Once Visual Studio is installed, click on C++ Build Tool. Select
        on the right column called "installation details" the following
        packages:
        -   MSCV v142 -- VS 2019
        -   Windows 10 SDK
        -   C++ CMake tools for Windows
        -   Testing Tools Core Feature
    -   Click install on the bottom right to download & install those
        packages
-   Install Rust, to be downloaded
    [here](https://www.rust-lang.org/tools/install)
-   Install Git for windows, to be downloaded
    [here](https://git-scm.com/download/win)

## Clone the Massa Git Repository

-   Open Windows Power Shell
    -   If the massa folder does not exist type:
        `git clone https://gitlab.com/massalabs/massa.git`
    -   Type: `cd massa`
    -   Type: `git stash`
    -   Type: `git checkout testnet`
    -   Type: `git pull`
-   Change default Rust to nightly
    -   Type: `rustup default nightly`

## Launch your Node

-   Open Windows Power Shell
    -   Type: `cd /massa`
    -   Type: `cd /massa-node`
    -   Type: `cargo run --release`

Your node is running. Once the process is finished, you should leave the
Powershell opened.

You can now launch your client and move toward the wallet creation then
the staking process.

## Launch your Client

-   Open Windows Power Shell
    -   Type: `cd /massa`
    -   Type: `cd /massa-client`
    -   Type: `cargo run --release`
