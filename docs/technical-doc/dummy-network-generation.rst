========================
Local network generation
========================

How to launch a local network with custom settings and initial coins & rolls repartition
========================================================================================

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
    wallet_generate_private_key

For the rest of the tutorial we will use theses abreviations:

- `PRIVK` : The private key you just generated
- `PUBK` : The public key corresponding to PRIVK
- `ADDR` : The address corresponding to PUBK

Setup your node to use the private key you just generated as its private key and staking key:
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

Your sandbox network containing your node will start after 10 seconds. You can now interact with it through the CLI client in the same way you would with a testnet node.
If you want to run multiple nodes on your local network you should use our advanced simulator. All the documentation is here : https://github.com/massalabs/massa-network-simulator

**On Docker**

Full documentation about launching a local network on Docker is available here : https://github.com/massalabs/massa-network-simulator