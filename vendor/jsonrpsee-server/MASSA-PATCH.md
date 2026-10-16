# Massa local JSON-RPC error-classification patch

Base: crates.io `jsonrpsee-server` 0.24.9, MIT. Original license and package
metadata are retained. `UPSTREAM.json` records the original crate checksum
and source hashes.

The only modified upstream file is `src/server.rs`: its local `prepare_error`
checks JSON syntax before extracting a request ID, so valid JSON with an
invalid envelope returns Invalid Request (-32600), while malformed JSON
returns Parse Error (-32700). Valid IDs are retained when available.

Regression tests live in Massa's API transport tests. No registry sources
are modified and no dependency version is upgraded. Remove this override
once an upstream release contains a verified equivalent fix.
