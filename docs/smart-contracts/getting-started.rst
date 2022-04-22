.. _sc-getting-started:

Getting started
===============

In this section you will learn how to compile your first Massa smart contract.

Setting up a new project
^^^^^^^^^^^^^^^^^^^^^^^^
.. collapse:: Wait, I know npm. Just give me the commands!

    Here you go:

   .. code-block:: shell

       npm init
       npm install --global yarn npx
       npm install --save-dev @as-pect/cli massa-sc-std
       npx asinit .
       npx asp --init

.. collapse:: OK, but I would like to use docker. Can you help me?

   Sure, no problem:

   .. code-block:: shell

       docker run -it --user $(id -u):$(id -g) -v $PWD:/app -v $PWD/.npm:/.npm -v $PWD/.config:/.config -w /app node:17.7-alpine npm init
       docker run --user $(id -u):$(id -g) -v $PWD:/app -v $PWD/.npm:/.npm -v $PWD/.config:/.config -w /app node:17.7-alpine npm install --save-dev @as-pect/cli
       docker run -it --user $(id -u):$(id -g) -v $PWD:/app -v $PWD/.npm:/.npm -v $PWD/.config:/.config -w /app node:17.7-alpinenpx asinit .
       docker run --user $(id -u):$(id -g) -v $PWD:/app -v $PWD/.npm:/.npm -v $PWD/.config:/.config -w /app node:17.7-alpine npx asp --init


Make sure you have a recent version of Node.js and npm. Update or `install <https://docs.npmjs.com/downloading-and-installing-node-js-and-npm>`_ them if needed.
Create or go to the directory where you want to set up your project and run:

.. code-block:: shell

   npm init
   npm install --global yarn npx


Don't bother with the different question, you can change all that by editing the `package.json` file.

.. note::
   You can add the parameter `--yes` to automatically set the default values.

Now that the npm project is created, install the `as-pect` and `massa-sc-std` dependencies by executing the following command:

.. code-block:: shell

   npm install --save-dev @as-pect/cli massa-sc-std massa-sc-scripts

You have now installed AssemblyScript and as-pect modules. The first one will be used to generate bytecode from AssemblyScript code and the second one will let you perform unit tests.

.. note::
    * Massa smart contract module (massa-sc-std) contains the API you need to use to interact with the external world of the smart contract (the node, the ledger...).
    * Installing directly as-pect will automatically install the compatible version of AssemblyScript.
    * All dependencies should be installed as dev dependencies as no module will be exported on this project.

Now that AssemblyScript and as-pect dependencies are installed, you can finish setting up your project by running:

.. code-block:: shell

   npx asinit .
   npx asp --init

Congratulations! Now you have a fully set up project and you are ready to add some code.

.. note::
   A few words on project folders:

    * `assembly` is where the code goes;
    * `assembly/__test__/` store the unit tests;
    * `build` will contain the compiled smart contracts.

Create your first smart contract
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

Since the beginning of mankind, humans explain how to use a program, a new language, a service by implementing a *Hello world!*.

Your first smart contract will be no exception!

.. note::

   I'm told that it has nothing to do with the beginning of mankind but Brian Kernighan used it for the first time in *a tutorial introduction to the language B* published in 1972.

Create and open a new file called `helloworld.ts` in `assembly` directory at the root of your project. In this file, write or copy/paste the following code:

.. code-block:: typescript

    import { print } from "massa-sc-std";

    export function main(_args: string): void {
        print("Hello world!");
    }

Don't forget to save the file. Before starting compilation, just a few words to describe what you just wrote or pasted:

* line 1: `print` function is imported from Massa API (massa-sc-std). This function will write to stdout the string given as argument.
* line 3: `main` function is exported. This means that the main function will be callable from the outside of the WebAssembly module (more about that later).
* line 4: `print` function is called with "Hello world!". Brian, we are thinking of you!

Now that everything is in place, we can start the compilation step by running the following command:

.. code-block:: shell

   npx massa-sc-scripts build-sc assembly/helloworld.ts

Congratulations! You have generated your first smart contract: the `helloworld.wasm` file in `build` directory.

.. note::

   If due to bad luck you have an error at compilation time:

   * check that you properly followed all the steps,
   * do a couple a internet research,
   * look for any similare issue (open or closed) in `this <//TODO>`_ project.

   If you find nothing, feel free to contact us at `TODO <//TODO>`_ or directly open an issue `here <//TODO>`_.

Execute your smart contract on a node
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

To execute the smart contract you will need:

- A running node with a log level set to DEBUG.
- A client configured with an address having coins.
- A smart contract compiled in WebAssembly (see previous step).

Let's go!

Running a node in debug mode
""""""""""""""""""""""""""""

Make sure that you have the last version of the Massa node. If not, `install it <https://github.com/massalabs/massa/wiki/install>`_.

Once the node is installed and before running it, set your log level to DEBUG:

- open `massa-node/base_config/config.toml` file with your preferred editor;
- set the log level to `3`.

.. note::

   Your file should look like the following:

    .. code-block:: toml

        [logging]
        # Logging level. High log levels might impact performance. 0: ERROR, 1: WARN, 2: INFO, 3: DEBUG, 4: TRACE
        level = 3

.. warning::
   A node configured with a verbose log level will write a lot of logs. You should restore default value as soon as this tutorial is finished.


Now that the node is properly configured to log `print` function output you have to `run the node <https://github.com/massalabs/massa/wiki/run>`_.

Configure the client
""""""""""""""""""""

If you don't have any wallet configured yet, `create a new one <https://github.com/massalabs/massa/wiki/wallet>`_.

If you're using a brand new wallet, add some coins by sending your address to `testnet-faucet discord channel <https://discord.com/channels/828270821042159636/866190913030193172>`_.

If you are using an existing wallet, make sure that you have at least 1 coin on it.

In any case, keep the `address` of your wallet, you will use it later.

Execute the smart contract on the node
""""""""""""""""""""""""""""""""""""""

Everything is in place, we can now execute the `hello world` smart contract on your local node with the following command inside the **client cli**:

.. code-block:: shell

   send_smart_contract <address> <path to wasm file> 0 0 0 0

.. note::

   We are executing the send_smart_contract command with 6 parameters:

   - <address>: the address of your wallet kept during previous step;
   - <path to wasm file>: the full path (from the root directory to the file extension .wasm) of the hello smart contract generated in the previous chapter.
   - Four 0 parameters that can be safely ignored by now. If you want more info on them, use the command `help send_smart_contract`.


If everything went fine, the following prompted message should be prompted:

.. code-block:: shell

   Sent operation IDs:
   <id with numbers and letters>

In that case, return to the node tab and have a look at the log, you should see a message like the following:

.. code-block:: shell

   <date and time> DEBUG massa_execution_worker::interface_impl: SC print: Hello world!


Congratulations! You have just executed your first smart contract on a node !!

Don't forget to change node's log level to INFO (value is 2).

Store a smart contract in the blockchain
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

TODO
