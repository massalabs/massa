.. index:: api

.. _technical-api:

==================
Massa JSON-RPC API
==================

This crate exposes Rust methods (through the `Endpoints` trait) as
JSON-RPC API endpoints (thanks to the `ParityJSON-RPC <https://github.com/paritytech/jsonrpc>`_ crate).

**E.g.** this curl command will call endpoint `node_stop` (and stop the
locally running `massa-node`):

.. code-block:: bash

    curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0", "method": "node_stop", "id": 123 }' 127.0.0.1:33034

You can interact with the Massa RPC API on `OpenRPC Playground <https://playground.open-rpc.org/?schemaUrl=https://raw.githubusercontent.com/massalabs/massa/main/docs/technical-doc/openrpc.json>`_.

Endpoints are organized in 2 authorizations levels:

**Public** API
==============

_a.k.a. **"user mode"** endpoints (running by default on `[::]:33035`)_

`get_status`
------------

Summary of the current state: time, last final blocks (hash, thread,
slot, timestamp), clique count, connected nodes count.

-   No parameters.

-   Return:

.. code-block:: javascript

    {
    "config": {
        "block_reward": String, // represent an Amount in coins
        "delta_f0": Number, // Used to compute finality threshold
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
    }

`get_cliques`
-------------

Get cliques.

-   No parameters.

-   Return:

.. code-block:: javascript

    [
        {
            block_ids: [String],
            fitness: Number,
            is_blockclique: Boolean,
        },
    ];

`get_stakers`
-------------

Returns the active stakers and their roll counts for the current cycle.

-   No parameters.

-   Return:

.. code-block:: javascript

    [
    "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx": Number
    ] // Dictionnary associating staker addresses to their active roll counts

`get_operations`
----------------

Returns operations information associated to a given list of operations' IDs.

-   Parameters:

.. code-block:: javascript

    [String]. // String must be an operation Id

-   Return:

.. code-block:: javascript

    [
    {
        "id": String, // Operation id
        "in_blocks": [String], // Block ids
        "in_pool": Boolean,
        "is_final": Boolean,
        "operation": {
        "content": {
            "expire_period": Number,// after that period, the operation become invalid forever
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
            OR
            "ExecuteSC" {
                "data": [Number], // vec of bytes to execute
                "max_gas": Number, // maximum amount of gas that the execution of the contract is allowed to cost.
                "coins": String, // represent an Amount in coins that are spent by consensus and are available in the execution context of the contract.
                "gas_price": String, // represent an Amount in coins, price per unit of gas that the caller is willing to pay for the execution.
            }
            OR
            "CallSC": {
                "target_addr": String, // Address
                "target_func": String, // Function name
                "param": String, // Parameter to pass to the function
                "max_gas": Number,
                "sequential_coins": Number, // Amount
                "parallel_coins": Number, // Amount
                "gas_price": Number, // Amount
            }
            },
            "sender_public_key": String
        },
        "signature": String
        }
    }
    ]

`get_endorsements`
------------------

Get endorsements

-   Parameters:

.. code-block:: javascript

    [String]; // string must be an endorsement id

-   Return:

.. code-block:: javascript

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

`get_block`
-----------

Get information on a block given its hash.

-   Parameters:

.. code-block:: javascript

    [String]; // Block IDs

-   Return:

.. code-block:: javascript

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
                    OR
                    ExecuteSC {
                        "data": [Number], // vec of bytes to execute
                        "max_gas": Number, // maximum amount of gas that the execution of the contract is allowed to cost.
                        "coins": String, // represent an Amount in coins that are spent by consensus and are available in the execution context of the contract.
                        "gas_price": String, // represent an Amount in coins, price per unit of gas that the caller is willing to pay for the execution.
                    }
                    OR
                    "CallSC": {
                        "target_addr": String, // Address
                        "target_func": String, // Function name
                        "param": String, // Parameter to pass to the function
                        "max_gas": Number,
                        "sequential_coins": Number, // Amount
                        "parallel_coins": Number, // Amount
                        "gas_price": Number, // Amount
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

`get_graph_interval`
--------------------

Get the block graph within the specified time interval.

-   Parameters:

.. code-block:: javascript

    {
        "start": null or Number, // in millis since 1970-01-01, field may be omitted
        "end": null or Number,// in millis since 1970-01-01, field may be omitted
    }

-   Return:

.. code-block:: javascript

    [
        {
            creator: String, // public key
            id: String, // Block Id
            is_final: Boolean,
            is_in_blockclique: Boolean,
            is_stale: Boolean,
            parents: [String], // as many block Ids as there are threads
            slot: {
                period: Number,
                thread: Number,
            },
        },
    ];

`get_datastore_entries`
--------------------

Get a data entry both at the latest final and candidate executed slots for the given addresses.

If an existing entry did not undergo any changes in the speculative history, it will return its final value in `candidate_value` field. If it was deleted in the active history, it will return null in `candidate_value` field.

-   Parameters:

.. code-block:: javascript

    [
        {
            "address": String,
            "key": String,
        }
    ];

-   Return:

.. code-block:: javascript

    [
        {
            "candidate_value": Byte array or null,
            "final_value": Byte array or null,
        }
    ]


`get_addresses`
---------------

Get addresses.

-   Parameters:

.. code-block:: javascript

    [
        [String], // Addresses
    ];

-   Return:

.. code-block:: javascript

    [
        {
            address: String,
            balance: {
                candidate_balance: String, // represent an Amount in coins
                final_balance: String, // represent an Amount in coins
                locked_balance: String, // represent an Amount in coins
            },
            block_draws: [
                {
                    period: Number,
                    thread: Number,
                },
            ],
            blocks_created: [String], // Block ids
            endorsement_draws: [
                {
                    slot: {
                        period: Number,
                        thread: Number,
                    },
                    index: Number,
                },
            ],
            involved_in_endorsements: [String], // Endorsement Id
            involved_in_operations: [String], // Operation id
            production_stats: [
                // as many items as cached cycles
                {
                    cycle: Number,
                    is_final: Boolean,
                    nok_count: Number,
                    ok_count: Number,
                },
            ],
            rolls: {
                active_rolls: Number,
                candidate_rolls: Number,
                final_rolls: Number,
            },
            thread: Number,
            final_balance_info: null OR Number,
            candidate_balance_info: null OR Number,
            final_datastore_keys: [Byte array],
            candidate_datastore_keys: [Byte array],
        },
    ];

`send_operations`
-----------------

Adds operations to pool. Returns operations that were ok and sent to
pool.

-   Parameters:

.. code-block:: javascript

    [[
        {
            "serialized_content": ByteArray,
            "creator_public_key": String,
            "signature": String
        }
    ]]

The `serialized_content` parameter contains all the content encoded in byte compact (see https://github.com/massalabs/massa/blob/main/massa-models/src/operation.rs#L185).
For the signature you need to use the bytes of the public_key and content in byte compact concatenated and sign it with ed25519.

Here is an example of the content format :

.. code-block:: javascript

    {
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
            OR
            "ExecuteSC": {
                "data": [Number], // vec of bytes to execute
                "max_gas": Number, // maximum amount of gas that the execution of the contract is allowed to cost.
                "coins": String, // represent an Amount in coins that are spent by consensus and are available in the execution context of the contract.
                "gas_price": String, // represent an Amount in coins, price per unit of gas that the caller is willing to pay for the execution.
            }
            OR
            "CallSC": {
                "target_addr": String, // Address
                "target_func": String, // Function name
                "param": String, // Parameter to pass to the function
                "max_gas": Number,
                "sequential_coins": Number, // Amount
                "parallel_coins": Number, // Amount
                "gas_price": Number, // Amount
            }
        },
    }


-   Return:

.. code-block:: javascript

    [String], // Operation ids

`get_filtered_sc_output_event`
------------------------------

Returns events optionally filtered by: start slot, end slot, emitter address, original caller address, operation id

It will take the interval `start slot..=end slot`

-   Parameters:

.. code-block:: javascript

    {
        "start": null OR {
                "period": Number, // will use by default Slot(0,0)
                "thread": Number // will use by default Slot(0,0)
            },
        "end": null OR {
                "period": Number, // will use by default Slot(0,0)
                "thread": Number // will use by default Slot(0,0)
            },
        "emitter_address": null OR String, // Address
        "original_caller_address": null OR String, // Address
        "original_operation_id": null OR String, // operation id
    }

-   Return:

.. code-block:: javascript

    [{
        "data": String, // Arbitrary json string generated by the smart contract
        "id": String // event id 
        "context":{
            "slot": {
                "period": Number,
                "thread": Number
            },
            "block": null OR String // block id,
            "read_only": Boolean // wether the event was generated during  read only call
            "call_stack": [String], //Addresses
            "index_in_slot": Number, 
            "origin_operation_id": null OR String // operation id
        }
    }]

`execute_read_only_call`
------------------------

Call a function of a contract in a read only context. The changes on the ledger will not be applied and directly drop after the context of the execution. All the events generated will be returned :

-   Parameters:

.. code-block:: javascript

    [{
        "max_gas": Number,
        "simulated_gas_price": Number,
        "target_address": String,
        "target_function": String,
        "parameter": String,
        "caller_address": String OR null,
    }]

-   Return:

.. code-block:: javascript

    [{
        "executed_at": {
        "period": Number,
        "thread": Number
        },
        "result": String, //"ok" or error message
        "output_events": [
        // Each id is a event id. The size of this array is dynamic over the number of events pop in the execution.
        "id1": {
            "id": String, //id of the event
            "context": {
            "slot": {
                "period": Number,
                "thread": Number
            },
            "block": null OR String // block id,
            "read_only": Boolean // wether the event was generated during  read only call
            "call_stack": [String], //Addresses
            "index_in_slot": Number,
            "origin_operation_id": null OR String // operation id
            }
            "data": String // String of the event you sended
        }
        ]
    }]


`execute_read_only_bytecode`
----------------------------

Execute a smart contract in a read only context. The changes on the ledger will not be applied and directly drop after the context of the execution. All the events generated will be returned :

-   Parameters:

.. code-block:: javascript

    [{
        "max_gas": Number,
        "simulated_gas_price": Number,
        "bytecode": [Number],
        "address": String OR null,
    }]

-   Returns:

.. code-block:: javascript

    [{
        "executed_at": {
        "period": Number,
        "thread": Number
        },
        "result": String, //"ok" or error message
        "output_events": [
        // Each id is a event id. The size of this array is dynamic over the number of events pop in the execution.
        "id1": {
            "id": String, //id of the event
            "context": {
            "slot": {
                "period": Number,
                "thread": Number
            },
            "block": null OR String // block id,
            "read_only": Boolean // wether the event was generated during  read only call
            "call_stack": [String], //Addresses
            "index_in_slot": Number,
            "origin_operation_id": null OR String // operation id
            }
            "data": String // String of the event you sended
        }
        ]
    }]

**Private** API
===============

_a.k.a. **"manager mode"** endpoints (running by default on `127.0.0.1:33034`)_

`node_stop`
-----------

Gracefully stop the node.

-   No parameters.

-   No return.

`node_sign_message`
-------------------

Sign message with node's key.

-   Parameter:

.. code-block:: javascript

    [u8];

-   Return:

.. code-block:: javascript

    {"public_key": String, "signature": String}

Where public_key is the public key used to sign the input and signature,
the resulting signature.

`node_add_staking_secret_keys`
------------------------------

Add a vec of new secret keys for the node to use to stake.

-   Parameter:

.. code-block:: javascript

    [String];

The strings must be secret keys.

-   No return.

`node_remove_staking_addresses`
-------------------------------

Remove a vec of addresses used to stake.

-   Parameter:

.. code-block:: javascript

    [String];

The strings must be addresses.

-   No return.

`node_get_staking_addresses`
----------------------------

Return hashset of staking addresses.

-   No parameters.

-   Return:

.. code-block:: javascript

    [String];

The strings are addresses.

`node_ban_by_ip`
----------------

Ban given IP address(es).

-   Parameter:

.. code-block:: javascript

    [String];

The strings must be IP address(es).

-   No return.

`node_ban_by_id`
----------------

Ban given id(s)

-   Parameter:

.. code-block:: javascript

    [String];

The strings must be node id(s).

-   No return.

`node_unban_by_ip`
------------------

Unban given IP address(es).

-   Parameter:

.. code-block:: javascript

    [String];

The strings must be IP address(es).

-   No return.

`node_unban_by_id`
------------------

Unban given id(s)

-   Parameter:

.. code-block:: javascript

    [String];

The strings must be node id(s)

-   No return.

`node_whitelist`
----------------

Whitelist given IP address(es).

-   Parameter:

.. code-block:: javascript

    [String];

The strings must be IP address(es).

-   No return.


`node_remove_from_whitelist`
----------------------------

Remove from whitelist given IP address(es).

-   Parameter:

.. code-block:: javascript

    [String];

The strings must be IP address(es).

-   No return.
