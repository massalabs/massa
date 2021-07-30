# Test config

## Requirements
Make sure you have [everything](https://gitlab.com/massalabs/massa/-/blob/main/README.adoc) installed. Agree on the branch to test and pull it.

Open needed ports. (For now we use 31244 for common messages and 31245 for bootstrap, but it can change.)

## Values to update in `config.toml`
* genesis_timestamp: set it to [current timestamp](https://www.epochconverter.com/) + delta
* current_node_index: the index to get your consensus keys. You have to agree with people you are testing. One index should represent one node. 
* (nodes):  if there are more than 4 nodes, generate enough keys for everyone
* (routable_ip): your ip if it is routable, else nothing. 
* (bootstrap_addr): the address you want to bootstrap from at startup. If no address is provided the node will start generating nodes with genesis blocks as parents
* (bind): where to listen if you allow people to bootstrap from you
* (bootstrap_public_key): make sure it is consistent with bootstrap node's private key.

## `peers.json`
Ensure that every one appears in at least one `peers.json` file, to ensure every one will be discovered.

Here is a valid file example.
```json
[
  {
    "advertised": true,
    "banned": false,
    "bootstrap": false,
    "ip": "ex.am.ple.ip",
    "last_alive": 1617375403310,
    "last_failure": 1617377610695
  }
]
```

## Generated files
`node_private.key` file and `block_store` folder are generated if missing. 

If you are a bootstrap node, start with a `node_private.key` file and make sure that you update `bootstrap_public_key` to be consistent with the private key.

## Test
First `cd massa-node` folder and `cargo run`. Then in another terminal `cd massa-client` and `cargo run`. And have fun.