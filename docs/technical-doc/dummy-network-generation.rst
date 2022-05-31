========================
Dummy network generation
========================

How to launch a dummy network with custom settings and initial coins & rolls repartition
========================================================================================

* get a private key and associated public key and address (see wallet_generate_private_key command), these will be referenced as PRI PUB and ADR
* replace `massa-node/base_config/initial_rolls.json` content with

.. code-block:: javascript

    {
        "ADR": 100
    }

* replace `massa-node/base_config/initial_ledger.json` content with

.. code-block:: javascript

    {
        "ADR": {
            "balance": "250000000"
        }
    }

* replace `massa-node/base_config/initial_sce_ledger.json` content with

.. code-block:: javascript

    {
        "ADR": 250000000
    }

* add or replace `massa-node/config/staking_keys.json`

.. code-block:: javascript

    [
    "PRI"
    ]

* Then do `cargo run --features sandbox` in massa-node folder
