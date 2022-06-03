========================
Local network generation
========================

How to launch a local network with custom settings and initial coins & rolls repartition
========================================================================================

**On your OS**

Clone massa:

.. code-block:: bash

    git clone git@github.com:massalabs/massa.git

Compile it with sandbox feature enabled:

.. code-block:: bash

    cd massa && cargo build --release --features sandbox

Create a keypair in massa-client:

.. code-block:: bash

    cd massa-client && cargo run
    wallet_generate_private_key

For the rest of the tutorial we will use theses abreviations:

- `PRIVK` : The private key you just generated
- `PUBK` : The public key corresponding to PRIVK
- `ADDR` : The address corresponding to PUBK

Setup your node to use the private key you just generate as its private key and staking key:
 * modify or create the file `massa-node/config/node_privkey.key` :

 .. code-block:: bash

    PRIVK

 * modify or create the file `massa-node/config/staking_keys.json` :

 .. code-block:: javascript

    [
        PRIVK
    ]

 * modify the file `massa-node/base_config/initial_ledger.json` :

 .. code-block:: javascript

    "ADDR": {
        "balance": "250000000"
    }

 * modify the file `massa-node/base_config/initial_rolls.json` :

 .. code-block:: javascript

    {
        "ADDR": 100
    }

 * modify the file `massa-node/base_config/initial_sce_ledger.json` :

 .. code-block:: javascript

    "ADDR": {
        "balance": "250000000"
    }

You can now launch your node :

  .. code-block:: bash
    
    cd massa-node && cargo run --features sandbox

The network with your node all start in 10 seconds and you can now interact it with the CLI client like a testnet node.
If you want to run multiple nodes on your local network you need to use the simulator that use docker all the documentation is here : https://github.com/massalabs/massa-network-simulator

**On Docker**

Full documentation about launching a local network on Docker is available here : https://github.com/massalabs/massa-network-simulator