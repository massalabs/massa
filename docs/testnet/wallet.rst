=======================
Creating a Massa wallet
=======================

A Massa wallet is a file that contains a list of your private keys.

Like other blockchains, Massa uses elliptic curve cryptography for the
security of your coins (with `secp256k1`).

It means your private key is your password allowing you to spend coins
that were sent to your address (your address is the hash of your public
key).

Here is how to create a Massa wallet.

From the command line interface
===============================

If your client is not running
-----------------------------

Go to the client folder:

.. code-block:: bash

    cd massa/massa-client/

Start the interactive client and load a wallet file:

.. code-block:: bash

    cargo run

It loads the wallet file `wallet.dat`. If it does not exist, it is created.

If your client is running
-------------------------

Now you can either generate a new private key (and associated public key/address):

.. code-block::

    wallet_generate_private_key

<u>**Or, if you already have one from a previous wallet**</u>, you can add manually an existing private key:

.. code-block::

    wallet_add_private_keys <your_private_key>

The list of addresses and keys of your wallet can be accessed with:

.. code-block::

    wallet_info

From the graphical interface
============================

If you don't plan to stake or use the command-line client, you can also
create a wallet on the web interface: head to the
`explorer <https://test.massa.net>`_, in the wallet tab.

Click `Generate private key` then `Add`.

This generates a new random private key from your computer randomness
which stays on your side, it is never transferred on the network.

Now you can add more addresses or see the list of your addresses with
their associated thread and balance.

Also, if you want to save this wallet and be able to restore it later,
click `Save wallet`, and next time directly do `Load wallet`.
