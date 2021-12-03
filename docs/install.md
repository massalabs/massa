# Installing a node

Right now 4 cores and 8 GB of RAM should be enough to run a node, but it might increase in the future. More info in the [FAQ](faq.md).

## From binaries
If you just wish to run a Massa node without compiling it yourself, you
can simply get the latest binary (download below and go the the next step: [Running a node](run.md))

-   [Windows
    executable](https://github.com/massalabs/massa/releases/latest/download/release_windows.zip)
-   [Linux
    binary](https://github.com/massalabs/massa/releases/latest/download/release_linux.zip)
-   [MacOS
    binary](https://github.com/massalabs/massa/releases/latest/download/release_macos.zip)

## From source code
Otherwise, if you wish to run a Massa node from source code, here are the steps to follow:

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
    `git clone --branch testnet https://github.com/massalabs/massa.git`

### On Windows

#### Set up your Rust environment

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

#### Clone the Massa Git Repository

-   Open Windows Power Shell
    - To clone the latest distributed version, type: `git clone --branch testnet https://github.com/massalabs/massa.git`

-   Change default Rust to nightly
    -   Type: `rustup default nightly`

### Next step

-   [Running a node](run.md)
