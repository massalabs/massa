.. _sc-getting-started:

Getting started
===============

In this section you will learn how to compile your first Massa smart contract.

Setting up a new project
^^^^^^^^^^^^^^^^^^^^^^^^
.. collapse:: Wait, I know npm. Just give me the commands!

    Here you go:

   .. code-block:: shell

       npx massa-sc-create my-sc

Make sure you have a recent version of Node.js and npm. Update or `install <https://docs.npmjs.com/downloading-and-installing-node-js-and-npm>`_ them if needed.
Create or go to the directory where you want to set up your project and run:

.. code-block:: shell

   npm install --global yarn npx
   npx massa-sc-create my-sc

Now that the npm project is created, go inside your smart-contract directory and install the dependencies using the following commands:

.. code-block:: shell

   cd my-sc
   npm install --legacy-peer-deps

You have now installed AssemblyScript among other dependencies. It will be used to generate bytecode from AssemblyScript code.

.. note::
    * Massa smart contract module (@massalabs/massa-sc-std) contains the API you need to use to interact with the external world of the smart contract (the node, the ledger...).
    * Installing directly as-pect will automatically install the compatible version of AssemblyScript.

Congratulations! Now you have a fully set up project and you are ready to add some code.

.. note::
   A few words on project folders:

    * `src` is where the code goes;
    * `build` will be created during compilation and will contain the compiled smart contracts.

Create your first smart contract
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

Since the beginning of mankind, humans explain how to use a program, a new language, a service by implementing a *Hello world!*.

Your first smart contract will be no exception!

.. note::

   I'm told that it has nothing to do with the beginning of mankind but Brian Kernighan used it for the first time in *a tutorial introduction to the language B* published in 1972.

Open the `main.ts` file in the `src` directory at the root of your project. Replace the code in the file by the following code:

.. code-block:: typescript

   import { generateEvent } from "@massalabs/massa-sc-std";

   export function main(_args: string): void {
        generateEvent("Hello world!");
    }

Don’t forget to save the file. Before starting compilation, just a few words to describe what is used here:

* line 1: `generateEvent` function is imported from Massa API (@massalabs/massa-sc-std). This function will generate an event with the string given as argument. Events can be later recovered using a Massa client.
* line 3: `main` function is exported. This means that the main function will be callable from the outside of the WebAssembly module (more about that later).
* line 4: `generateEvent` function is called with "Hello world!". Brian, we are thinking of you!

Now that everything is in place, we can start the compilation step by running the following command:

.. code-block:: shell

   npm run build

Congratulations! You have generated your first smart contract: the `main.wasm` file in `build` directory.

.. note::

   If due to bad luck you have an error at compilation time:

   * check that you properly followed all the steps,
   * do a couple a internet research,
   * look for any similare issue (open or closed) in `this <https://github.com/massalabs/massa-sc-std/>`_ project.

   If you find nothing, feel free to contact us on `Discord <https://discord.gg/massa>`_ or directly open an issue `here <https://github.com/massalabs/massa-sc-std/>`_.

Execute your smart contract on a node
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

To execute the smart contract you will need:

- A client configured with an address having coins.
- A smart contract compiled in WebAssembly (see previous step).

Let's go!

Configure the client
""""""""""""""""""""

Make sure that you have the last version of the Massa node. If not, `install it <https://github.com/massalabs/massa/wiki/install>`_.

If you don't have any wallet configured yet, `create a new one <https://github.com/massalabs/massa/wiki/wallet>`_.

If you're using a brand new wallet, add some coins by sending your address to `testnet-faucet discord channel <https://discord.com/channels/828270821042159636/866190913030193172>`_.

If you are using an existing wallet, make sure that you have some coins on it.

In any case, keep the `address` of your wallet, you will use it later.

Execute the smart contract on the node
""""""""""""""""""""""""""""""""""""""

Everything is in place, we can now execute the `hello world` smart contract on your local node with the following command inside the **client cli**:

.. code-block:: shell

   send_smart_contract <address> <path to wasm file> 100000 0 0 0

.. note::

   We are executing the send_smart_contract command with 6 parameters:

   - <address>: the address of your wallet kept during previous step;
   - <path to wasm file>: the full path (from the root directory to the file extension .wasm) of the hello smart contract generated in the previous chapter.
   - 100000: the maximum amount of gas that the execution of your smart-contract is allowed to use.
   - Three 0 parameters that can be safely ignored by now. If you want more info on them, use the command `help send_smart_contract`.

If everything went fine, the following prompted message should be prompted:

.. code-block:: shell

   Sent operation IDs:
   <id with numbers and letters>

In that case, you should be able to retrieve the event with the `Hello world` emited. Use the following command inside the **client cli**:

.. code-block:: shell

   get_filtered_sc_output_event operation_id=<id with numbers and letters>

If everything went well you should see a message similar to this one:

.. code-block:: shell

   Context: Slot: (period: 627, thread: 22) at index: 0
   On chain execution
   Block id: VaY6zeec2am5i1eKKPzuyvhbzxVU8mts7ykSDj5usHyobJee8
   Origin operation id: wHGoVbp8QSwWxEMzM5nK9CpKL3SpNmxzUF3E4pHgn8fVkJmR5
   Call stack: A12Lkz8mEZ4uXPrzW9WDo5HKWRoYgeYjiQZMrwbjE6cPeRxuSfAG

   Data: Hello world!

Congratulations! You have just executed your first smart contract !

In the next tutorial you'll see a more involved example showing you how to create a Tictactoe smart-contract.