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
       npm install --save-dev @as-pect/cli
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

   npm install --save-dev @as-pect/cli massa-sc-std

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

   yarn run asc assembly/helloworld.ts --target release --exportRuntime --binaryFile build/helloworld.wasm

Congratulations! You have generated your first smart contract: the `helloworld.wasm` file in `build` directory.

.. note::

   If due to bad luck you have an error at compilation time:

   * check that you properly followed all the steps,
   * do a couple a internet research,
   * look for any similare issue (open or closed) in `this <//TODO>`_ project.

   If you find nothing, feel free to contact us at `TODO <//TODO>`_ or directly open an issue `here <//TODO>`_.

Add your smart contract to the blockchain
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

In this section you will learn how to deploy your first Massa smart contract.

First of all, check if your node setup and wallet are correct.

    .. collapse:: About your node :
    
        Modify the logs of your node to the level 3 in order to see the messages from the massa SC runtime :

        Open a terminal and be sure you are in the massa directory (with `cd massa` command) and edit the file the `nano` command (as following)

        .. code-block:: shell
            nano massa-node/base_config/config.toml

        You can now edit the `config.toml` file and you have to replace level = 2 by level = 3 like : 

        .. code-block:: shell

            [logging]
                level = 3

        Then press Ctrl + X to save, and press "Y" then `enter` to validate.
        
        Run your node to check the modification :
        
        ..code-block:: shell
            cd massa/massa-node
            RUST_BACKTRACE=full cargo run --release |& tee logs.txt
            
        As a consequence of the logs level 3, you should now get from the node not only `INFO` messages as previously but also the `DEBUG` messages like:
    
        capture.jpg

        .. note::

           If your node is not installed yet follow the steps : https://github.com/massalabs/massa/wiki/install
           
    .. collapse:: About your wallet :
        (be sure your node is running, if not follow : https://github.com/massalabs/massa/wiki/run)
        To create a wallet follow this tutorial : https://github.com/massalabs/massa/wiki/wallet
        You will need coins to deploy the smart contrat, send your `address` to the faucet bot in the "testnet-faucet" channel of our Discord (https://discord.com/invite/massa).
        
        ..note::
            If you don't know where to find your `address`, run the client :
            ..code-block:: shell
                cd massa/massa-client
                cargo run --release
            and use the command `wallet_info` into the client.
       
When all the setup steps about your node and wallet are done, just start your node : (if your node is already running because of the setup steps, don't start a new one) :
    
..code-block:: shell
    cd massa/massa-node
    RUST_BACKTRACE=full cargo run --release |& tee logs.txt
    
.. Note::

    To check if your node is running properly or not, write `get_status` command into the client, you will get : 
    
    ..code-block::
        ✔ command · get_status
        Error: check if your node is running: error trying to connect: tcp connect error: Connection refused (os error 111)
     
    If you have the error message, just restart your node. If it still doesn't work you can find help on https://discord.com/invite/massa, in #testnet channel
    
Next, you have to copy manually and paste your `helloword.wasm` from the previous `build` directory, to the `massa-client` directory (`massa/massa-client`)
    
When your node is running, open a second terminal and run the client :

..code-block:: shell
    cd massa/massa-client
    cargo run --release
    

Into the client, use the command send_smart_contract :

..code-blocki:: shell
    send_smart_contract your_address your_file.wasm 1000000 0 0 0

Command : send_smart_contract 
Parameters : SenderAddress PathToBytecode MaxGas GasPrice Coins Fee

.. note::

    You should get the following message printed into the client : 
    
    ..code-block:: shell
        ✔ command · send_smart_contract your_address your_file.wasm 1000000 0 0 0
        Sent operation IDs: operation_id
        
Your `helloworld.wasm` file will be executed by the EVM on the Massa blockchain, and you can find the print into the `DEBUG` messages of your node switching to your node window and Ctrl+F to find : `SC Print`

..code-block:: shell

    DEBUG massa_execution_worker::interface_impl: SC print: Hello world!

capture2
