.. index:: decentralized web, browser plugin

.. _web-plugin:

Massa's browser plugin
======================

In order to access decentralized websites we've developed a `browser plugin <https://github.com/massalabs/massa-wallet>`_.

The Massa browser plugin function like Metamask (wallet, interaction with the blockchain) but also provide extra functions:

* when an URL of the form `xxxxxxxxxxxx.massa` is typed in the address bar, the plugin will check if `xxxxxxxxxxxx` is an address
* if it's an address, the plugin will try to load the `website.zip` file from the filestore of that address, unzip it, and display its index.html (or whatever other page is requested).
* if it's not and address but something like a domain name, the plugin will interrogate a "Massa Name Service" smart contract through a readonly call to try to match the domain name to an address. This is inspired by how the Ethereum Name Service works. Then it will load the address's website as defined above.
* the website will typically contain html, css and javascript to remain lightweight. The javascript can dynamically talk to the Massa plugin to interact with the blockchain and wallet (just like Metamask)
* if the website needs heavy assets (videos, large images...) that cannot be stored on the blockchain, the plugin allows looking up optional external data on IPFS or on the normal web, but plugin settings allow users to disable the fetching of off-chain assets for maximum security.

Installation
------------

For now the Massa browser plugin is only compatible with Chrome and Firefox.

To install it, you can follow this installation procedure detailed on the
`Github repository <https://github.com/massalabs/massa-wallet>`_.
