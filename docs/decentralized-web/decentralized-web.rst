.. index:: decentralized web

.. _web-intro:

Rationale
=========

The *code is law* rule is a cornerstone of DeFi. It states among other things that once successfully audited
, a program can remain trusted. This implies that the program of a successfully audited smart contract may
never be unexpectedly changed by an outsider (note that contracts that modify their own code during runtime
cannot be trusted this way in the first place, but this is visible in the code audit). Popular ETH smart contracts
essentially follow that rule.

However, most DeFi web3 apps such as `app.uniswap.org <https://app.uniswap.org/>`_ are typically used through
an otherwise normal website that talks to a browser plugin (typically `Metamask <https://github.com/MetaMask/metamask-extension>`_)
allowing the webpage to interact with the user's wallet and the blockchain. The website that serves as an
entry point to the dapp is neither decentralized nor immutable-once-audited, thus breaking the very foundations of DeFi
security. And that's how you get into situations like `this one <https://www.theverge.com/2021/12/2/22814849/badgerdao-defi-120-million-hack-bitcoin-ethereum>`_.

The goal here is to allow addresses on Massa to store not only a balance, bytecode and a datastore, but also named files.
Those files must remain small, or storing them will cost a lot to their owner. Any address, through bytecode execution,
can initialize, read and write the "files" part just like it would with the datastore. The reason why we don't reuse the
datastore for this, outside of the risk of key collisions, is for easier auditing: if the code never writes into its own
bytecode nor its filestore after deployment, it is safe to assume that the stored website can't change anymore.
That's it from the point of view of the node.

Decentralized websites deployed on the Massa blockchain can then be accessed in your browser
using a browser extension :ref:`here <web-plugin>`.

That way, Massa allows deploying fully decentralized code-is-law apps, as it was meant to be !

To close the loop, we can imagine dumping a copy of the source code of Massa and surrounding tools in the filestore of an on-chain smart contract.

Filestore
---------

From the point of the node, this functions just like another extra binary datastore in the SCE ledger, but indexed by a max-255-char string (instead of hash) and we call it the "filestore".
