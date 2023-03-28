## Intro

Massa tools:
* setup_test: cargo script to retrieve compiled wasm files (for unit tests)

## Script: setup_test

Download archive from massa-unit-tests-src repository, extract archive and copy:
* wasm files

Note that this script is run in the CI (see [ci.yml](../.github/workflows/ci.yml))

### Manual Run

#### Setup

cargo install cargo-script

* https://github.com/DanielKeep/cargo-script

If required, please update the Git tag in setup_test.rs (line 25)

#### Run

* cargo script setup_test.rs

### Run with local sources

* cargo script setup_test.rs -- --local "../massa-unit-tests-src/build/massa/*.wasm"

### Howto: add a new SC unit tests

* Clone: https://github.com/massalabs/massa-unit-tests-src
* Add your unit tests under:
  * assembly/massa/TEST_NAME/main.ts OR
  * assembly/massa-sc-runtime/TEST_NAME.ts
* Add your script entry in package.json, convention is:
  * build:TEST_NAME (for massa)
  * build:rt:TEST_NAME (for massa-sc-runtime)
* Test with: npm run build (and check that your test has been compiled)
* Git push && tag
  * tag name should follow: TEST.X.Y (ex: TEST.15.0, TEST.15.1, ...)
    * where X: is the testnet number
    * where Y: is the incremental version for this testnet
* In massa repo, edit [tools/setup_test.rs](setup_test.rs) to update the line:
  * const TAG: &str = "..."
  * run the script
