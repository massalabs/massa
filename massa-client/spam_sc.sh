#!/usr/bin/env bash

for VAR in {1..1}
do
    max_gas=$((1000000 + VAR))
    cargo run --release send_smart_contract 2KziQHMiHmmU3juWPvCYQr1hV3ns8NkXaV3WKH6wV3jycD7SVE ../../tmp/createSC.wasm $max_gas 0 0 0
done
