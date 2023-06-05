#!/usr/bin/env bash

for VAR in {1..100000}
do
    max_gas=$((1000000 + VAR))
    ../target/release/massa-client send_smart_contract xh1fXpp7VuciaCwejMF7ufF19SWv7dFPJ7U6HiTQaeNEFBiV3 ../../cryptocat/build/createSC.wasm $max_gas 0 0  0
    sleep 0.01
done
