# Massa local JSON-RPC protocol patch

Base: crates.io `jsonrpsee-server` 0.24.9, MIT. Original license and package
metadata are retained. `UPSTREAM.json` records the original crate checksum
and source hashes.

The protocol changes are in `src/server.rs` and
`src/middleware/rpc/layer/rpc_service.rs`. The local `prepare_error`
checks JSON syntax before extracting a request ID, so valid JSON with an
invalid envelope returns Invalid Request (-32600), while malformed JSON
returns Parse Error (-32700). Valid IDs are retained when available.

The notification decoder also rejects every present `id`, so unsupported ID
types produce Invalid Request instead of being silently treated as notifications
in single requests and batches. Supported ID-bearing requests retain precedence.

Valid ordinary notifications now execute through the normal service exactly once,
with their extensions preserved and their responses suppressed. Batch members
must be objects before dispatch. The internal notification marker prevents
id-less subscription handshakes from creating an orphan stream. Existing
body, response and batch limits remain in force.

For Rust 1.91.1 with warnings denied, `src/server.rs` also removes redundant
trait-object parentheses and `src/utils.rs` makes inferred return lifetimes
explicit. Comment typos are corrected in `src/future.rs` and
`src/transport/http.rs`. These adjustments preserve runtime behavior.

Regression tests live in Massa's API transport tests. No registry sources
are modified and no dependency version is upgraded. Remove this override
once an upstream release contains a verified equivalent fix.
