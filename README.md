## Setup from scratch for ubuntu

* curl : `sudo apt install curl`
* [rustup](https://www.rust-lang.org/tools/install) : `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
* configure path: `source $HOME/.cargo/env`
* check rust version: `rustc --version`
* install [nightly](https://doc.rust-lang.org/edition-guide/rust-2018/rustup-for-managing-rust-versions.html): `rustup toolchain install nightly`
* set it as default: `rustup default nightly`
* check rust version: `rustc --version`
* install git: `sudo apt install git`
* clone this repo: `git clone https://gitlab.com/massalabs/massa-network.git`
* `sudo apt install build-essential`
* `sudo apt install libssl-dev`