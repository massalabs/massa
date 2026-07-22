# WMAS bytecode patch artifact

`wmas_patched.wasm` must contain the reproducible build of the fixed WMAS
contract (from massa-standards). It is embedded via `include_bytes!` in
`src/wmas_patch.rs` and applied once at the activation slot (strategy 1).

It is intentionally EMPTY in this commit so the crate builds while the patch is
inert (gated behind a MIP that does not exist yet). Replace it, set
`WMAS_ADDRESS`, and register the activation MIP before enabling.
