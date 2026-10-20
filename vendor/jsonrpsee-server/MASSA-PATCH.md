# Massa local JSON-RPC error-classification patch

Base: crates.io `jsonrpsee-server` 0.24.9, MIT. Original license and package
metadata are retained. `UPSTREAM.json` records the original crate checksum
and source hashes.

In `src/server.rs`, the local `prepare_error`
checks JSON syntax before extracting a request ID, so valid JSON with an
invalid envelope returns Invalid Request (-32600), while malformed JSON
returns Parse Error (-32700). Valid IDs are retained when available.

For Rust 1.91.1 with warnings denied, `src/server.rs` also removes redundant
trait-object parentheses and `src/utils.rs` makes inferred return lifetimes
explicit. Comment typos are corrected in `src/future.rs` and
`src/transport/http.rs`. These adjustments preserve runtime behavior.

Regression tests live in Massa's API transport tests. No registry sources
are modified and no dependency version is upgraded. Remove this override
once an upstream release contains a verified equivalent fix.
