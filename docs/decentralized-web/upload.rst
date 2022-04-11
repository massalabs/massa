======================
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
