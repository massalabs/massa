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
  "algo_config": {
    "block_reward": String, // represent an Amount in coins
    "delta_f0": Number, // Used to compute finality thresold
    "end_timestamp": null or Number, // millis since 1970-01-01 (only in tesnets)
    "genesis_timestamp": Number, // millis since 1970-01-01
    "operation_validity_periods": Number,
    "periods_per_cycle": Number,
    "pos_lock_cycles": Number,
    "pos_lookback_cycles": Number,
    "roll_price": String, // represent an Amount in coins
    "t0": Number, // millis between to slots in the same thread
    "thread_count": Number
  },
  "connected_nodes": {
    "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx": String // Node id -> ip address
  },
  "consensus_stats": {
    "clique_count": Number,
    "end_timespan": Number,// stats time interval, millis since 1970-01-01
    "final_block_count": Number,
    "final_operation_count": Number,
    "staker_count": Number,
    "stale_block_count": Number,
    "start_timespan": // stats time interval, millis since 1970-01-01
  },
  "current_cycle": Number,
  "current_time": Number, // millis since 1970-01-01
  "delta_f0": Number, // Used to compute finality thresold
  "genesis_timestamp": Number, // millis since 1970-01-01
  "last_slot": {
    "period": Number,
    "thread": Number
  },
  "network_stats": {
    "active_node_count": Number,
    "banned_peer_count": Number,
    "in_connection_count": Number,
    "known_peer_count": Number,
    "out_connection_count": Number
  },
  "next_slot": {
    "period": Number,
    "thread": Number
  },
  "node_id": String,
  "node_ip": null or String, // ip address if provided
  "pool_stats": {
    "endorsement_count": Number,
    "operation_count": Number
  },
  "roll_price": String, // represent an Amount in coins
  "t0": Number,
  "thread_count": Number,
  "version": String
}

```

### `get_cliques`

Get cliques.

- No parameters.

- Return:

```javascript
[
  {
    "block_ids": [String],
    "fitness": Number,
    "is_blockclique": Boolean
  }
]
```

### `get_stakers`

Returns the active stakers and their roll counts for the current cycle.

- No parameters.

- Return:

```javascript
{
  "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx": Number
} // Dictionnary associating staker addresses to their active roll counts
```

### `get_operations`

Returns operations information associated to a given list of operations' IDs.

- Parameters:

```javascript
[String]. // String must be an operation Id
```

- Return:

```javascript
[
  {
    "id": String, // Operation id
    "in_blocks": [String], // Block ids
    "in_pool": Boolean,
    "is_final": Boolean,
    "operation": {
      "content": {
        "expire_period": Number,
        "fee": String, // represent an Amount in coins
        "op": {
          "Transaction": {
            "amount": String, // represent an Amount in coins
            "recipient_address": String
          }
          OR
          "RollBuy": {
            "roll_count": Number
          }
          OR
          "RollSell": {
            "roll_count": Number
          }
        },
        "sender_public_key": String
      },
      "signature": String
    }
  }
]

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
    "in_pool": Boolean,
    "in_blocks": [String], // BlockId,
    "is_final": Boolean,
    "endorsement": {
        "content":{
            "sender_public_key": String,
            "slot": {
                "period": Number,
                "thread": Number
            },
            "index": Number,
            "endorsed_block": String // BlockId,
        }
        "signature": String
    }
 }]
```

### `get_block`

Get information on a block given its hash.

- Parameters:

```javascript
String // Block id
```

- Return:

```javascript
{
    "id": String // BlockId,
    "content": Null or {
        "is_final": bool,
        "is_stale": bool,
        "is_in_blockclique": bool,
        "block": {
            "header": {
                "content": {
                  "endorsed_block": String, // Block id
                  "index": Number,
                  "sender_public_key": String, 
                  "slot": { // endorsed block slot: deifferent from block's slot
                    "period": Number,
                    "thread": Number
                  }
                },
                "signature": String
              }
            ],
            "operation_merkle_root": String, // Hash of all operations
            "parents": [String], // Block ids, as many as thread count
            "slot": {
              "period": Number,
              "thread": Number
            }
          },
          "signature": String
        },
        "operations": [
          {
            "content": {
              "expire_period": Number,
              "fee": String, // represent an Amount in coins
              "op": {
                "Transaction": {
                  "amount": String, // represent an Amount in coins
                  "recipient_address": String
                }
                OR
                "RollBuy": {
                  "roll_count": Number
                }
                OR
                "RollSell": {
                  "roll_count": Number
                }
              },
              "sender_public_key": String
            },
            "signature": String
          }
        ]
      },
      "is_final": Boolean,
      "is_in_blockclique": Boolean,
      "is_stale": Boolean
    },
}
```

### `get_graph_interval`

Get the block graph within the specified time interval.

- Parameters:

```javascript
{
    "start": null or Number, // in millis since 1970-01-01, field may be omitted
    "end": null or Number,// in millis since 1970-01-01, field may be omitted
}
```

-   Return:

```javascript
[
  {
    "creator": String, // public key
    "id": String, // Block Id
    "is_final": Boolean,
    "is_in_blockclique": Boolean,
    "is_stale": Boolean,
    "parents": [String], // as many block Ids as there are threads
    "slot": {
      "period": Number,
      "thread": Number
    }
  }
]
```

### `get_addresses`

Get addresses.

- Parameters:

```javascript
[
  [String] // Addresses
]

```

- Return:

```javascript
[
  {
    "address": String,
    "balance": {
      "candidate_balance": String, // represent an Amount in coins
      "final_balance": String, // represent an Amount in coins
      "locked_balance": String // represent an Amount in coins
    },
    "block_draws": [
      {
        "period": Number,
        "thread": Number
      },
    ],
    "blocks_created": [String], // Block ids
    "endorsement_draws": [
      {
        "slot": {
          "period": Number,
          "thread": Number
        },
        "index": Number
      }
    ],
    "involved_in_endorsements": [String], // Endorsement Id
    "involved_in_operations": [String], // Operation id
    "production_stats": [ // as many items as cached cycles
      {
        "cycle": Number,
        "is_final": Boolean,
        "nok_count": Number,
        "ok_count": Number
      }
    ],
    "rolls": {
      "active_rolls": Number,
      "candidate_rolls": Number,
      "final_rolls": Number
    },
    "thread": Number
  }
]

```

### `send_operations`

Adds operations to pool. Returns operations that were ok and sent to
pool.

- Parameters:

```javascript
[[
  {
    "content": {
      "expire_period": Number,
      "fee": String, // represent an Amount in coins
      "op": {
        "Transaction": {
          "amount": String, // represent an Amount in coins
          "recipient_address": String
        }
        OR
        "RollBuy": {
          "roll_count": Number
        }
        OR
        "RollSell": {
          "roll_count": Number
        }
      },
      "sender_public_key": String
    },
    "signature": String
  }
]]
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
