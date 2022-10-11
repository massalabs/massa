## Intro

## Script: setup_test

Download archive from massa-unit-tests-src repository, extract archive and copy:
* wasm files
* sources files

Note that this script is manually run in the CI (see [ci.yml](../.github/workflows/ci.yml))

### Setup

cargo install cargo-script

* https://github.com/DanielKeep/cargo-script

If required, please update the Git tag in setup_test.rs (line 25)

### Run

* cargo script setup_test
