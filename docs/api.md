# Massa JSON-RPC API

This crate exposes Rust methods (through the [`Endpoints` trait) as
JSON-RPC API endpoints (thanks to the [Parity
JSON-RPC](https://github.com/paritytech/jsonrpc) crate).

**E.g.** this curl command will call endpoint `stop_node` (and stop the
locally running `massa-node`):

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0", "method": "stop_node", "id": 123 }' 127.0.0.1:33034
```

Endpoints are organised in 2 authorisations levels:

## **Public** API, a.k.a. *"user mode"* endpoints (running by default on `[::]:33035`)

### `get_status`

Summary of the current state: time, last final blocks (hash, thread,
slot, timestamp), clique count, connected nodes count.

- No parameters.

- Return:

```javascript
{
    "node_id": String, // identifies the node
    "node_ip": String or Null, // node's ip address, if provided
    "version": String,
    "genesis_timestamp": Number, // start of the network, in millis since 1st january 1970
    "t0": Number, // number of millis between two slots in a thread (depends on thread_count)
    "delta_f0": Number, // used to compute finality threshold
    "roll_price": Number, // price in coins of a roll
    "thread_count": Number, // number of threads in the network
    "current_time": Number, // current time in millis since 1st january 1970
    "connected_nodes": Object{NodeId, IpAddr},
    "last_slot": Null or {"period": Number, "thread", Number}, // last slot if avaible
    "next_slot": {"period": Number, "thread", Number},
    "consensus_stats": {
        "start_timespan": Number, // start in millis since 1st january 1970 of that measurement
        "end_timespan": Number, // end in millis since 1st january 1970 of that measurement
        "final_block_count": Number,
        "final_operation_count": Number,
        "stale_block_count": Number,
        "clique_count": Number,
    },
    "pool_stats": {
        "operation_count": Number,
        "endorsement_count": Number,
    },
    "network_stats": {
        "in_connection_count": Number,
        "out_connection_count": Number,
        "known_peer_count": Number,
        "banned_peer_count": Number,
        "active_node_count": Number,
     },
     "algo_config": {
        "genesis_timestamp": Number,
        "end_timestamp": Null or Number,
        "thread_count": Number,
        "t0": Number,
        "delta_f0": Number,
        "operation_validity_periods": Number,
        "periods_per_cycle": Number,
        "pos_lookback_cycles": Number, // Proof of Stake lookback cycles: when drawing for cycle N, we use the rolls from cycle N - pos_lookback_cycles - 1
        "pos_lock_cycles": Number, // Proof of Stake lock cycles: when some rolls are released, we only credit the coins back to their owner after waiting  pos_lock_cycles
        "block_reward": Amount,
        "roll_price": Amount,
    },
}
```

### `get_cliques`

Get cliques.

- No parameters.

- Return:

```javascript
[{
       "block_ids": [String], // strings are block ids
       "fitness": Number,
       "is_blockclique": Bool,
 }, ... ].
```

### `get_stakers`

Returns the active stakers and their roll counts for the current cycle.

- No parameters.

- Return:

```javascript
[Address: {
    "active_rolls": Number, // rolls that are taken in account right now
    "final_rolls": Number, // rolls according to last final blocks
    "candidate_rolls": Number, // rolls acccording to last blocks
 }, ... ]
```

### `get_operations`

Returns operations information associated to a given list of operations' IDs.

- Parameters:

```javascript
[String]. // String must be an operation Id
```

- Return:

```javascript
[{
    "id": String, // string is an OperationId,
    "in_pool": bool,
    "in_blocks": [String], // string is a BlockId,
    "is_final": bool,
    "operation": {
        "content":
            "sender_public_key": String // string is a PublicKey,
            "fee": Number, // in coins
            "expire_period": Number,
            "op": OperationType, // TODO not sure how this go in JSON
        }
        "signature": String,
    }
 }, ... ].
```

### `get_endorsements`

Get endorsements (not yet implemented)

- Parameters:

```javascript
[String] // string must be an endorsement id
```

- Return:

```javascript
[{
    "id": String, // EndorsementId,
    "in_pool": bool,
    "in_blocks": [String], // BlockId,
    "is_final": bool,
    "endorsement": {
        "content":{
            "sender_public_key": PublicKey,
            "slot": {"period": Number, "thread", Number},
            "index": Number,
            "endorsed_block": String // BlockId,
        }
        "signature": String
    }
 }, ... ]
```

### `get_blocks`

Get information on a block given its hash.

- Parameters:

```javascript
[String] // Block ids
```

- Return:

```javascript
[{
    "id": String // BlockId,
    "content": Null or {
        "is_final": bool,
        "is_stale": bool,
        "is_in_blockclique": bool,
        "block": {
            "header": {
                "content": {
                    "creator": String // PublicKey,
                    "slot": {"period": Number, "thread", Number},
                    "parents": [String] // BlockId,
                    "operation_merkle_root": String, // all operations hash
                    "endorsements": [{
                        "content":{
                            "sender_public_key": PublicKey,
                            "slot": {"period": Number, "thread", Number},
                            "index": Number,
                            "endorsed_block": String // BlockId,
                        }
                        "signature": String
                     }, ... ],
                },
                "signature": Signature,
            },
            operations: [{
                "content": {
                    "sender_public_key": String // string is a PublicKey,
                    "fee": Number, // in coins
                    "expire_period": Number,
                    "op": OperationType, // TODO not sure how this go in JSON
                }
                "signature": String,
             }, ... ],
        },
    },
 }, ... ]
```

### `get_graph_interval`

Get the block graph within the specified time interval.

- Parameters:

```javascript
{
    "start": Null or Number, // in millis since 1970-01-01
    "end": Null or Number,// in millis since 1970-01-01
}
```

-   Return:

```javascript
[{
    "id": String // BlockId,
    "is_final": bool,
    "is_stale": bool,
    "is_in_blockclique": bool,
    "slot": {"period": Number, "thread", Number},
    "creator":  String // Address,
    "parents": [String] // BlockId,
 }, ... ]
```

### `get_addresses`

Get addresses.

- Parameters:

```javascript
[String]
```

- Return:

```javascript
[{
    "address": String // Address,
    "thread": Number,
    "balance": {
        "final_balance": Number,
        "candidate_balance": Number,
        "locked_balance": Number,
     },
     "rolls": {
        "active_rolls": Number,
        "final_rolls": Number,
        "candidate_rolls": Number,
     },
     "block_draws": [{"period": Number, "thread", Number }, ... ],
     "endorsement_draws": {Slot: Number}, // number is the index
     "blocks_created": [String], // Block ids
     "involved_in_endorsements": [String], // Endorsement ids,
     "involved_in_operations": [String], // Operation ids,
     "is_staking": bool,
 }, ... ]
```

### `send_operations`

Adds operations to pool. Returns operations that were ok and sent to
pool.

- Parameters:

```javascript
[{
    "content": {
        "sender_public_key": String // string is a PublicKey,
        "fee": Number, // in coins
        "expire_period": Number,
        "op": OperationType, // TODO not sure how this go in JSON
    }
    "signature": String,
  }, ... ]
```

-   Return:

```javascript
[String], // Operation ids
```

## **Private** API, a.k.a. *"manager mode"* endpoints (running by default on `127.0.0.1:33034`)

### `stop_node`

Gracefully stop the node.

-   No parameters.

-   No return. 

### `node_sign_message`

Sign message with node's key.

-   Parameter:

```javascript
[u8]
```

-   Return:

```javascript
{"public_key": String, "signature": String}
```

Where public_key is the public key used to sign the input and signature,
the resulting signature. 

### `add_staking_private_keys`

Add a vec of new private keys for the node to use to stake.

-   Parameter:

```javascript
[String]
```

The strings must be private keys.

-   No return. 

### `remove_staking_addresses`

Remove a vec of addresses used to stake.

-   Parameter:

```javascript
[String]
```

The strings must be addresses.

-   No return. 

### `get_staking_addresses`

Return hashset of staking addresses.

-   No parameters.

-   Return:

```javascript
[String]
```

The strings are addresses. 

### `ban`

Bans given IP addresses.

-   Parameter:

```javascript
[String]
```

The strings must be ip addresses.

-   No return. 

### `unban`

Unbans given IP addresses.

-   Parameter:

```javascript
[String]
```

The strings must be ip addresses.

-   No return.
