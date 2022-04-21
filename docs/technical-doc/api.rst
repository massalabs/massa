.. index:: api

.. _technical-api:

==================
Massa JSON-RPC API
==================

This crate exposes Rust methods (through the `Endpoints` trait) as
JSON-RPC API endpoints (thanks to the `ParityJSON-RPC <https://github.com/paritytech/jsonrpc>`_ crate).

**E.g.** this curl command will call endpoint `stop_node` (and stop the
locally running `massa-node`):

.. code-block:: bash

    curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0", "method": "stop_node", "id": 123 }' 127.0.0.1:33034

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

    {
    "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx": Number
    } // Dictionnary associating staker addresses to their active roll counts

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
            sce_ledger_info : {
                balance: String // reprensents an amount
                module: null OR [Number] // stored bytecode
                datastore: [
                    xxxxxxxxxxxxxxxxxxxxxx: [Number] // bytes
                ]
            }
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
            "ExecuteSC": {
            "data": [Number], // vec of bytes to execute
            "max_gas": Number, // maximum amount of gas that the execution of the contract is allowed to cost.
            "coins": String, // represent an Amount in coins that are spent by consensus and are available in the execution context of the contract.
            "gas_price": String, // represent an Amount in coins, price per unit of gas that the caller is willing to pay for the execution.
            }
        },
        "sender_public_key": String
        },
        "signature": String
    }
    ]]

-   Return:

.. code-block:: javascript

    [String], // Operation ids

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

`get_sc_output_event_by_slot_range`
-----------------------------------

Returns output events by slot range. (not yet implemented)

-   Parameters:

.. code-block:: javascript

    {
        "start": {
                    "period": Number,
                    "thread": Number
                },
        "end": {
                    "period": Number,
                    "thread": Number
                }
    }

-   Return:

.. code-block:: javascript

    [
    {
        "data": String, // Arbitrary json string generated by the smart contract
        "context":{
            "slot": {
                "period": Number,
                "thread": Number
            },
            "block": null OR String // block id,
            "call_stack": [String], //Addresses
        }
    }
    ]

`get_sc_output_event_by_sc_address`
-----------------------------------

Returns output events by smart contract address. (not yet implemented)

-   Parameters:

.. code-block:: javascript

    String // Address

-   Return:

.. code-block:: javascript

    [
    {
        "data": String, // Arbitrary json string generated by the smart contract
        "context":{
            "slot": {
                "period": Number,
                "thread": Number
            },
            "block": null OR String // block id,
            "call_stack": [String], //Addresses
        }
    }
    ]

`get_sc_output_event_by_caller_address`
---------------------------------------

Returns output events by caller address. (not yet implemented)

-   Parameters:

.. code-block:: javascript

    String //Address

-   Return:

.. code-block:: javascript

    [
    {
        "data": String, // Arbitrary json string generated by the smart contract
        "context":{
            "slot": {
                "period": Number,
                "thread": Number
            },
            "block": null OR String // block id,
            "call_stack": [String], //Addresses
        }
    }
    ]


**Private** API
===============

_a.k.a. **"manager mode"** endpoints (running by default on `127.0.0.1:33034`)_

`stop_node`
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

`add_staking_private_keys`
--------------------------

Add a vec of new private keys for the node to use to stake.

-   Parameter:

.. code-block:: javascript

    [String];

The strings must be private keys.

-   No return.

`remove_staking_addresses`
--------------------------

Remove a vec of addresses used to stake.

-   Parameter:

.. code-block:: javascript

    [String];

The strings must be addresses.

-   No return.

`get_staking_addresses`
-----------------------

Return hashset of staking addresses.

-   No parameters.

-   Return:

.. code-block:: javascript

    [String];

The strings are addresses.

`ban`
-----

Bans given IP addresses.

-   Parameter:

.. code-block:: javascript

    [String];

The strings must be ip addresses.

-   No return.

`unban`
-------

Unbans given IP addresses.

-   Parameter:

.. code-block:: javascript

    [String];

The strings must be ip addresses.

-   No return.
