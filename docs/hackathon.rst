Documentation for the Kryptosphere Hackathon
============================================

Welcome to the main technical documentation for the Hackathon. You will find everything you need to develop this weekend.

First of all you need to have a client to interact with the node of the network for this weekend we have prepared some pre-build :

https://github.com/massalabs/massa/releases/tag/LABN.0.0

The zip will contains two folders one called `massa-node` and the other `massa-client`. We will only with the client in this tutorial.
In the folder `massa-client` you will find an executable `massa-client` that will be your client for the whole Hackathon.

When you have this client you can insert the secret key we gave you using this command in the client:

    wallet_add_secret_keys <secret_key>

Now you have created a wallet you can check the address, balance, etc... with this command:

    wallet_info

You should see coins on your address.
Now that you have an address you can create and deploy your first smart contract. 

Smart contracts
===============

Setting up your working environment
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

Install node and npm by following `this tutorial <https://heynode.com/tutorial/install-nodejs-locally-nvm/>`__ and make sure you have `yarn` and `npx` installed:

.. code-block:: shell

    npm install --global yarn npx


Discover your first smart contract
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

You can follow `this guide <smart-contracts/massa-sc-by-example.html>`__ to deploy and interact with a tic-tac-toe.

Create your smart contract
^^^^^^^^^^^^^^^^^^^^^^^^^^

Now that you have your first smart-contract and you know how to deploy and interact you can create your by using the template of an `hello-world` using this command:

.. code-block:: shell

    npx massa-sc-create massa-sc-template

Test your smart-contract
^^^^^^^^^^^^^^^^^^^^^^^^

You can test your smart-contract without publishing it on the blockchain by using `this tester <https://github.com/massalabs/massa-sc-tester>`__. You have a complete documentation on the README.md of the tester but to test specifically the tic-tac-toe you can run:

.. code-block:: shell

    cargo run path/to/deploy.wasm
    cargo run path/to/play.wasm


Create your frontend
^^^^^^^^^^^^^^^^^^^^

As you saw in the tec-tac-toe example, you can create a website to interact with the smart-contract using our `massa-web3 <https://github.com/massalabs/massa-web3>`__ library.

We have two example for websites:

- In JS you have the example of the `game of life <https://github.com/massalabs/massa-sc-examples/tree/main/games/game-of-life>`_
- In React you have the template `create-react-app-massa <https://github.com/massalabs/create-react-app-massa>`_

