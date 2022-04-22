.. index:: web; decentralized, decentralized web

.. _sc-decentralized-web:

=========================
Massa's decentralized web
=========================

Massa's decentralized web allows you to store websites directly on the blockchain. This feature provides another layer of security for your dApps.

This documentation go through all the steps to host your website on the blockchain and register it on Massa's DNS service in order to access it using Massa's plugin.

Uploading your website
======================

To upload a website to Massa's decentralized web you can use the following smart-contract. If you want latter want to be able to access to your website through the Massa plugin it's important to use the entry `massa_web` for your website.

.. code-block:: typescript

    import { include_base64, print, Storage } from "massa-sc-std";

    function createWebsite(): void {
        const bytes = include_base64('./site.zip');
        Storage.set_data("massa_web", bytes);
    }

    export function main(_args: string): i32 {
        createWebsite();
        print("Uploaded site");
        return 0;
    }

This smart-contract and the dependencies are available `here <https://github.com/massalabs/massa-sc-examples/tree/fix-8/website>`_.

Setting the DNS
===============

To claim a DNS address for you smart-contract you can use the following smart-contract:

.. code-block:: typescript

    import { call } from "massa-sc-std";
    import { JSON } from "json-as";

    @json
    export class SetResolverArgs {
        name: string = "";
        address: string = "";
    }

    export function main(_args: string): i32 {
        const sc_address = "2cVNfo79K173ddPwNezMi8WzvapMFojP7H7V4reCU2dk6QTeA"
        call(sc_address, "setResolver", JSON.stringify<SetResolverArgs>({name: "flappy", address: "Gr2aeZt7ZRb9S5SKgAEV1tZ6ERLHGhBCZsAp2sdB6i3rDK9M7"}), 0);
        return 0;
    }

For now DNS addresses can only be claimed using the following address: `9mvJfA4761u1qT8QwSWcJ4gTDaFP5iSgjQzKMaqTbrWCFo1QM` (and its associated private key).

Once you've done this step, you should be able to access to your website using `Massa's browser plugin <https://github.com/massalabs/massa-wallet>`_ at `massa://flappy`.
