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

1) copy and paste your `helloword.wasm` from the previous `build` directory, to the `massa-client` directory (`massa/massa-client`)
2) modify the logs of your node to the level 3 in order to see the messages from the massa SC runtime :

Open a terminal and be sure you are in the massa directory and edit the file the nano command (as following)

.. code-block:: shell
   cd massa
   nano massa-node/base_config/config.toml
   
You can now edit the config.toml file and you have to replace level = 2 by level = 3 like : 

.. code-block:: shell

   [logging]
    level = 3
    
Then press Ctrl + X to save, and press "Y" then `enter` to validate.

3) Run your node :

.. code-block:: shell

   cd massa/massa-node
   RUST_BACKTRACE=full cargo run --release |& tee logs.txt

If your node is not installed yet follow the steps : https://github.com/massalabs/massa/wiki/install

As a consequence of the logs level 3, you should now get not only `INFO` messages as previously from the node but also the `DEBUG` messages like :

.. code-block:: shell
   2022-04-11T21:49:34.612046Z  INFO massa_node: Node version : LABN.0.0
   2022-04-11T21:49:34.612335Z  INFO massa_bootstrap: Start bootstrapping from 51.75.131.129:31245
   2022-04-11T21:49:34.950386Z  INFO massa_bootstrap: Successful bootstrap
   2022-04-11T21:49:34.950571Z DEBUG massa_network_worker: starting network controller
   2022-04-11T21:49:34.951056Z  INFO massa_network_worker: The node_id of this node is: 63hPEVCkgPEEpgQ8PZUcgyRJTVJ6Q4kL8sFqSFM2vPbw75tSrH
   2022-04-11T21:49:34.951137Z DEBUG massa_network_worker: Loading peer database
   2022-04-11T21:49:34.951828Z DEBUG massa_network_worker: network controller started
   [...]
   2022-04-11T21:49:37.050423Z DEBUG massa_graph::block_graph: received header 2TShJa889bKk1jYEMdvme4hdy1CzZuagJuNZARaeVm3eYgVHdM for slot (period: 440, thread: 16)
   2022-04-11T21:49:37.560348Z DEBUG massa_graph::block_graph: received header cfC4rRJPy99nbX7GKY6zwoWHQQuLaWT1AdSWZKWwRbfTqpLC5 for slot (period: 440, thread: 17)
   2022-04-11T21:49:38.105549Z DEBUG massa_graph::block_graph: received header GYHd21sMPL9RQ6iy47Crjg39VpLKxSoPwLbyBhGm3am1WkS5R for slot (period: 440, thread: 18)
   2022-04-11T21:49:38.546567Z DEBUG massa_graph::block_graph: received header 2MK2DvjGjtJfmKsF8GWG8SCcKpviEnpbyF1xvaBpXDLniUpsMn for slot (period: 440, thread: 19)
   2022-04-11T21:49:39.059884Z DEBUG massa_graph::block_graph: received header 2JzChSc8wLmwkmEzME6pfufvzivyN1wD7VQEVQJBLQuj8T8jES for slot (period: 440, thread: 20)
   2022-04-11T21:49:39.557067Z DEBUG massa_graph::block_graph: received header SKXearKboqSc2qWW9429QXgT6SAJ6bvEuVhhaEE7wvWbWJmEK for slot (period: 440, thread: 21)
   2022-04-11T21:49:40.046741Z DEBUG massa_graph::block_graph: received header 2RGNFkta3PRAKQWaF7wjfJstczk4BYhLUAEY2kqST9J9z6qyLh for slot (period: 440, thread: 22)

4) Run the client :

..code-block:: shell
  cd massa/massa-client
  cargo run --release
  
when the client is launched you will see : 

..code-block:: shell

███    ███  █████  ███████ ███████  █████ 
████  ████ ██   ██ ██      ██      ██   ██
██ ████ ██ ███████ ███████ ███████ ███████
██  ██  ██ ██   ██      ██      ██ ██   ██
██      ██ ██   ██ ███████ ███████ ██   ██

Use 'exit' to quit the prompt
Use the Up/Down arrows to scroll through history
Use the Right arrow or Tab to complete your command
Use the Enter key to execute your command
HELP of Massa client (list of available commands):
- exit no args: exit the client gracefully
- help no args: display this help
- unban [IpAddr]: unban given IP addresses
- ban [IpAddr]: ban given IP addresses
- node_stop no args: stops the node
- node_get_staking_addresses no args: show staking addresses
- node_remove_staking_addresses Address1 Address2 ...: remove staking addresses
- node_add_staking_private_keys PrivateKey1 PrivateKey2 ...: add staking private keys
- node_testnet_rewards_program_ownership_proof Address discord_id: generate the testnet rewards program node/staker ownership proof
- node_whitelist [IpAddr]: whitelist given IP addresses
- node_remove_from_whitelist [IpAddr]: remove from whitelist given IP addresses
- get_status no args: show the status of the node (reachable? number of peers connected, consensus, version, config parameter summary...)
- get_addresses Address1 Address2 ...: get info about a list of addresses (balances, block creation, ...)
- get_block BlockId: show info about a block (content, finality ...)
- get_endorsements EndorsementId1 EndorsementId2 ...: show info about a list of endorsements (content, finality ...)
- get_operations OperationId1 OperationId2 ...: show info about a list of operations(content, finality ...) 
- wallet_info no args: show wallet info (private keys, public keys, addresses, balances ...)
- wallet_generate_private_key no args: generate a private key and add it into the wallet
- wallet_add_private_keys PrivateKey1 PrivateKey2 ...: add a list of private keys to the wallet
- wallet_remove_addresses Address1 Address2 ...: remove a list of addresses from the wallet
- wallet_sign Address string: sign provided string with given address (address must be in the wallet)
- buy_rolls Address RollCount Fee: buy rolls with wallet address
- sell_rolls Address RollCount Fee: sell rolls with wallet address
- send_transaction SenderAddress ReceiverAddress Amount Fee: send coins from a wallet address
- send_smart_contract SenderAddress PathToBytecode MaxGas GasPrice Coins Fee: create and send an operation containing byte code
- read_only_smart_contract PathToBytecode MaxGas GasPrice Address: execute byte code, address is optional. Nothing is really executed on chain
- read_only_call TargetAddress TargetFunction Parameter MaxGas GasPrice SenderAddress: call a smart contract function, sender address is optional. Nothing is really executed on chain
- when_episode_ends no args: show time remaining to end of current episode
- when_moon no args: tells you when moon
? command ›

Then, write `wallet_info` in order to check if a wallet is connected to your node.

You should get the following print result :

..code-block:: shell
✔ command · wallet_info
Private key: f9WuMS5Bsugkqa2T7kSVWjWqkciPqMHHZCqXtt2fGHLQSdEp4
Public key: 8TVsRirM4eThkt5SyRZHVPmgGfkzT2GomDGy5SQAFrqTWewXJi
Address: hBywzhNzPfvEXVftXxoVTv56AXX5FaFfg2sHwQSUXrTrzMjRk
Thread: 11
Final Sequential balance:
	Balance: 0

Candidate Sequential balance:
	Balance: 0

Parallel balance:
	Final balance: 0
	Candidate balance: 0
	Locked balance: 0

Rolls:
	Active rolls: 0
	Final rolls: 0
	Candidate rolls: 0


If you get :

✔ command · wallet_info
WARNING: do not share your private key

It is because your wallet is not connected. You have to follow this tutorial: https://github.com/massalabs/massa/wiki/wallet#if-your-client-is-running (since the step "if your client is running"). Next, you must have coins to send smart contract on the blockchain. You can get coins following this tutorial : https://github.com/massalabs/massa/wiki/staking

(the end tomorrow)

