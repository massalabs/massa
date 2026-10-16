# jsonrpsee

[![GitLab Status](https://gitlab.parity.io/parity/mirrors/jsonrpsee/badges/master/pipeline.svg)](https://gitlab.parity.io/parity/mirrors/jsonrpsee/-/pipelines)
[![crates.io](https://img.shields.io/crates/v/jsonrpsee)](https://crates.io/crates/jsonrpsee)
[![Docs](https://docs.rs/jsonrpsee/badge.svg)](https://docs.rs/jsonrpsee)
![MIT](https://img.shields.io/crates/l/jsonrpsee.svg)
[![CI](https://github.com/paritytech/jsonrpsee/actions/workflows/ci.yml/badge.svg)](https://github.com/paritytech/jsonrpsee/actions/workflows/ci.yml)
[![Benchmarks](https://github.com/paritytech/jsonrpsee/actions/workflows/benchmarks_gitlab.yml/badge.svg)](https://github.com/paritytech/jsonrpsee/actions/workflows/benchmarks_gitlab.yml)
[![dependency status](https://deps.rs/crate/jsonrpsee/latest/status.svg)](https://deps.rs/crate/jsonrpsee)

JSON-RPC library designed for async/await in Rust.

Designed to be the successor to [ParityTech's JSONRPC crate](https://github.com/paritytech/jsonrpc/).

## Features
- Client/server HTTP/HTTP2 support
- Client/server WebSocket support
- Client WASM support via web-sys
- Client transport abstraction to provide custom transports
- Middleware

## Documentation
- [API Documentation](https://docs.rs/jsonrpsee)

## Examples

- [HTTP](./examples/examples/http.rs)
- [WebSocket](./examples/examples/ws.rs)
- [WebSocket pubsub](./examples/examples/ws_pubsub_broadcast.rs)
- [API generation with proc macro](./examples/examples/proc_macro.rs)
- [CORS server](./examples/examples/cors_server.rs)
- [Core client](./examples/examples/core_client.rs)
- [HTTP proxy middleware](./examples/examples/http_proxy_middleware.rs)
- [jsonrpsee as service](./examples/examples/jsonrpsee_as_service.rs)
- [low level API](./examples/examples/jsonrpsee_server_low_level_api.rs)
- [Websocket served over dual-stack (v4/v6) sockets](./examples/examples/ws_dual_stack.rs)

See [this directory](./examples/examples) for more examples

## Roadmap

See [our tracking milestone](https://github.com/paritytech/jsonrpsee/milestone/2) for the upcoming stable v1.0 release.

## Users

If your project uses `jsonrpsee` we would like to know. Please open a pull request and add your project to the list below:
- [parity bridges common](https://github.com/paritytech/parity-bridges-common)
- [remote externalities](https://github.com/paritytech/substrate/tree/master/utils/frame/remote-externalities)
- [polkadot-sdk](https://github.com/paritytech/polkadot-sdk)
- [substrate-api-client](https://github.com/scs/substrate-api-client)
- [subwasm](https://github.com/chevdor/subwasm)
- [subway](https://github.com/AcalaNetwork/subway)
- [subxt](https://github.com/paritytech/subxt)
- [Trin](https://github.com/ethereum/trin)
- [Uptest](https://github.com/uptest-sc/uptest)
- [zkSync Era](https://github.com/matter-labs/zksync-era)
- [Forest](https://github.com/ChainSafe/forest)

## Benchmarks

Daily benchmarks for jsonrpsee can be found:
- Gitlab machine: <https://paritytech.github.io/jsonrpsee/bench/dev2>
