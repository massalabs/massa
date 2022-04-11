===============
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
