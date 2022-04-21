=======
Staking
=======

In Massa, the minimal number of coins required to be able to stake is
100 MAS (which is called one "roll"). The total initial supply is 500m
MAS, so in theory, there could be 5 million people staking.

Addresses are randomly selected to stake (create blocks) in all threads,
based on the number of rolls they possess. The list of stakers and their
rolls can be seen `there <https://test.massa.net/#staking>`_.

Rolls can be bought with Massa coins or sold to get the coins back. If
you already have more than 100 Massa, you can continue this tutorial,
otherwise, send your address to the faucet bot in the
"testnet-faucet" channel of our `Discord <https://discord.com/invite/massa>`_.

Buying rolls
============

Get the address that has coins in your wallet. In the Massa client:

.. code-block::

    wallet_info

Buy rolls with it: put your address, the number of rolls you want to
buy, and the operation fee (you can put 0):

.. code-block::

    buy_rolls <address> <roll count> <fee>

As an example, the command for buying 1 roll with 0 fee for the address `VkUQ5MA4niNBhAEP7uVf89tvPfUHcbgy6BrdLM9SAuFSyy9DE`
is: `buy_rolls VkUQ5MA4niNBhAEP7uVf89tvPfUHcbgy6BrdLM9SAuFSyy9DE 1 0`


It should take less than one minute for your roll to become final, check
with:

.. code-block::

    wallet_info

Telling your node to start staking with your rolls
==================================================

Get the private key that has rolls in your wallet:

.. code-block::

    wallet_info

Register your private key so that your node start to stake with it:

.. code-block::

    node_add_staking_private_keys <your_private_key>

Now you should wait some time so that your rolls become active: 3 cycles
of 128 periods (one period is 32 blocks - 16 sec), so about 1h40
minutes.

You can check if your rolls are active with the same command:

.. code-block::

    wallet_info

When your rolls become active, that's it! You're staking! Please note, having one
roll is enough. On the testnet, we don't value how many rolls you have, but how reliable is your node.

You should be selected to create blocks in the different threads.

To check when your address is selected to stake, run this command:

.. code-block::

    get_addresses <your_address>

and look at the "next draws" section.

Also check that your balance increases, for each block or endorsement that you
create you should get a small reward.

Selling rolls
=============

If you want to get back some or all of your coins, sell rolls the same
way you bought them:

.. code-block::

    sell_rolls <address> <roll count> <fee>

It should take some time again for your coins to be credited, and they
will be frozen for 1 cycle before you can spend them, again check with:

.. code-block::

    wallet_info
