# Ethereum JSON-RPC Specification Compatibility

API info at https://playground.open-rpc.org/?schemaUrl=https://raw.githubusercontent.com/ethereum/eth1.0-apis/assembled-spec/openrpc.json&uiSchema%5BappBar%5D%5Bui:splitView%5D=false&uiSchema%5BappBar%5D%5Bui:input%5D=false&uiSchema%5BappBar%5D%5Bui:examplesDropdown%5D=false

## Motivations & feature requests:

* [x] Use wrapper to simplify API filters https://gitlab.com/massalabs/massa/-/issues/48
* [ ] Do not use ApiEvent to interact with other controllers, use their command senders directly. This will improve performance and reduce programming overhead.
* [x] Use a shared structures crate for API <=> Client communication: https://docs.rs/jsonrpc-derive/18.0.0/jsonrpc_derive/
* [ ] Improve endpoints to factor better, reduce redundancy...
* [ ] It would be nice to be able to see to which cliques a block belong for visualisation purposes. from @qdr
* [ ] Clique indices are not consistent from one call to the other. However, for now we can indicate whether a block is in the blockclique or not. from @damip
* [ ] If possible it would be nice to have the history of blocks created by an address. People are asking for this feature. from @qdr

## Useful links

**Metamask.io**

- all methods: https://docs.metamask.io/guide/rpc-api.html#ethereum-json-rpc-methods
- important methods: https://metamask.github.io/api-playground/api-documentation/

**Ethereum JSON RPC**

- wiki: https://eth.wiki/json-rpc/API
- playground: https://playground.open-rpc.org/?schemaUrl=https://raw.githubusercontent.com/ethereum/eth1.0-apis/assembled-spec/openrpc.json&uiSchema%5BappBar%5D%5Bui:splitView%5D=true&uiSchema%5BappBar%5D%5Bui:input%5D=false&uiSchema%5BappBar%5D%5Bui:examplesDropdown%5D=false

## Ports used

3 types of interaction : admin, client and eth on different ports:

- metamask.io interface will be expose on port `31242` using https://docs.rs/jsonrpc-http-server
- massa client (debug + explorer + user) use 2 API running on ports `31243` (manager/admin) and `31244` (stats/info/user/wallet) using https://docs.rs/jsonrpc-core-client
- bootstrap run on port `31245`

## API endpoints list (I/O action and `eth_` mapping with our client)

Implementation -> https://gitlab.com/massalabs/massa/-/merge_requests/171/diffs
