========================
Local network generation
========================

How to launch a local network with custom settings and initial coins & rolls repartition
========================================================================================

.. _docker:

**On Docker**

Full documentation about launching a local network on Docker is available here : https://github.com/massalabs/massa-network-simulator

**On your OS**

Clone massa:

.. code-block:: bash

    git clone git@github.com:massalabs/massa.git

Compile it with the sandbox feature enabled:

.. code-block:: bash

    cd massa && cargo build --release --features sandbox

Create a keypair in massa-client:

.. code-block:: bash

    cd massa-client && cargo run
    wallet_generate_secret_key

For the rest of the tutorial we will use theses abreviations:

- `SECRETK` : The secret you just generated
- `PUBK` : The public key corresponding to SECRETK
- `ADDR` : The address corresponding to PUBK

Setup your node to use the secret you just generated as its public key and staking key:
 * modify or create the file `massa-node/config/node_privkey.key` :

 .. code-block:: bash

    {"secret_key":"SECRETK","public_key":"PUBK"}

 * modify the file `massa-node/base_config/initial_ledger.json` :

 .. code-block:: javascript
    {
        "ADDR": {
            "balance": "250000000"
        }
    }

 * modify the file `massa-node/base_config/initial_rolls.json` :

 .. code-block:: javascript

    {
        "ADDR": 100
    }

 * modify the file `massa-node/base_config/initial_sce_ledger.json` :

 .. code-block:: javascript
    
    {
        "ADDR": "250000000"
    }

You can now launch your node :

  .. code-block:: bash
    
    cd massa-node && cargo run --features sandbox

On your client run the following command to add your secret key as staking key:

.. code-block:: bash
        
    cd massa-client && cargo run node_add_staking_secret_keys SECRETK

The network with your node all start in 10 seconds and you can now interact it with the CLI client like a testnet node.
If you want to run multiple nodes on your local network you need to use :ref:`docker`.
