.. index:: tictactoe

.. _sc-example:

==================================
Massa's smart-contracts by example
==================================

.. note::

    This tutorial doesn't assume any existing knowledge of the Massa protocol.

In this tutorial, we will go through all the steps required to create a smart-contract on Massa. We will cover all the steps by building a small game on Massa: a decentralized Tic-tac-toe!

This tutorial is divided into several parts:

- :ref:`writing-sc` will show you how to write Massa smart-contracts
- :ref:`sending-sc` will show you how to send your smart-contract to the Massa blockchain
- :ref:`interacting` will show you how to interact with your smart-contract
- :ref:`dapp` will get you through the process of creating your first dApp
- :ref:`hosting` will show you how to host your dApp on Massa's decentralized web

You can find the complete project on this `Github repository <https://github.com/massalabs/massa-sc-examples/tree/main/games/tictactoe>`__.

Prerequisites
=============

Smart-contracts are written in `Assembly Script <https://www.assemblyscript.org/>`_, and so we’ll assume that you have some familiarity with it, but you should be able to follow along even if you’re coming from a different programming language. We’ll also assume that you’re familiar with programming concepts like functions, objects, arrays, and to a lesser extent, classes.

For the decentralized website part, we'll assume that you have some familiarity with HTML and JavaScript. If you want to have more details, you can follow `this great tutorial from React <https://reactjs.org/tutorial/tutorial.html>`_ from which the dApp is heavily inspired from.

.. _writing-sc:

Writing your smart-contract
===========================

Smart-contract in Massa are written in `Assembly Script <https://www.assemblyscript.org/>`_ and then compiled to `WebAssembly <https://webassembly.org/>`_ (WASM). We chose WebAssembly as it is efficient and can be compiled from several languages, including Assembly Script.

Setup
-----

You need `node`, `yarn` and `git` to initialize the project!

.. code-block:: shell

    npx massa-sc-create tictactoe-sc

Once this repository is cloned, run the following command in the freshly created directory:

.. code-block:: shell

    yarn install

This command will initialize a new folder with a hello-world smart-contract example.

Writing the smart-contract
--------------------------

Smart-contracts are in the `src` directory. We will write the tic-tac-toe smart-contract `smart-contract.ts` file. The `main.ts` file is used to create a smart-contract that is used to create the tic-tac-toe smart-contract on the Massa blockchain. It may be confusing right now, but we'll go through all these steps in the following.

smart-contract.ts
-----------------

Let's start with tic-tac-toe smart-contract. As the main goal of this tutorial is to understand how to use Massa's smart-contracts, we will not go through the details of each function.

.. code-block:: typescript

    /**
    * Initialize a new tictactoe game
    */
    import { Storage } from "massa-sc-std";
    import { JSON } from "json-as";

    export function initialize(_args: string): void {
        Storage.set_data("currentPlayer", "X");
        Storage.set_data("gameState", "n,n,n,n,n,n,n,n,n");
        Storage.set_data("gameWinner", "n");
    }

The `initialize` function is used to start a new tic-tac-toe game. This function is used to instantiate the different variables that will be used to track the state of the game: `currentPlayer`, `gameState` and `gameWinner`. Note that smart-contract data is stored in a hash map where keys and values must be string.

Notice that in this example, the `initialize` function is public (see the `export`). It means that anyone can call it. In a real-world example, you will probably want to design a more complex mechanism!

We now turn to the game logic:

.. code-block:: typescript

    @json
    export class PlayArgs {
        index: u32 = 0;
    }

    export function play(_args: string): void {
        const args = JSON.parse<PlayArgs>(_args);
        let game_winner = Storage.get_data("gameWinner");
        if (game_winner == "n") {
            let player = Storage.get_data("currentPlayer");
            let game_state = Storage.get_data("gameState");
            let vec_game_state = game_state.split(",");
            assert(args.index >= 0);
            assert(args.index < 9);
            if (vec_game_state[args.index] == "n") {
                vec_game_state[args.index] = player;
                Storage.set_data("gameState", vec_game_state.join());
                if (player == "X") {
                    Storage.set_data("currentPlayer", "O");
                }
                else {
                    Storage.set_data("currentPlayer", "X");
                }
                _checkWin(player)
            }
        }
    }

    function _checkWin(player: string): void {
        const winningConditions = [
            [0, 1, 2],
            [3, 4, 5],
            [6, 7, 8],
            [0, 3, 6],
            [1, 4, 7],
            [2, 5, 8],
            [0, 4, 8],
            [2, 4, 6]
        ];

        let game_state = Storage.get_data("gameState");
        let vec_game_state = game_state.split(",");

        let roundWon = false;
        for (let i = 0; i <= 7; i++) {
            const winCondition = winningConditions[i];
            let a = vec_game_state[winCondition[0]];
            let b = vec_game_state[winCondition[1]];
            let c = vec_game_state[winCondition[2]];
            if (a == "n" || b == "n" || c == "n") {
                continue;
            }
            if (a == b && b == c) {
                roundWon = true;
                break
            }
        }

        if (roundWon) {
            Storage.set_data("gameWinner", player);
        }

        let roundDraw = !vec_game_state.includes("n");
        if (roundDraw) {
            Storage.set_data("gameWinner", "draw");
        }
    }

The `play` function is used to update the state of the game when each player plays. As the `initialize` function, it is a public function: anyone can call it and play the next move. Public functions of Massa smart-contracts can only take strings as arguments. To pass several arguments, we thus have to rely on `json-as` and to define the possible arguments using `PlayArgs`.

The `_checkWin` function is used to check whether the game ended or not. This function is private, as it does not use the `export` prefix. As such, it cannot be executed by external calls and can only be called internally by the smart-contract.

main.ts

.. code-block:: typescript

    import { generate_event, include_base64, create_sc } from "massa-sc-std";

    function createContract(): string {
        const bytes = include_base64('./build/smart-contract.wasm');
        const sc_address = create_sc(bytes);
        return sc_address;
    }

    export function main(_args: string): i32 {
        const sc_address = createContract();
        generate_event("Created tictactoe smart-contract at:" + sc_address);
        return 0;
    }

Compiling your smart-contract
-----------------------------

Smart-contract can be compiled using the `massa-sc-scripts` command: `yarn run build`.

.. _sending-sc:

Putting your smart-contract on the blockchain
=============================================

We'll now turn to the process of putting the smart-contract on the Massa blockchain.

Sending the smart-contract
==========================

Sending the smart-contract to the Massa blockchain is done using the `send_smart_contract` from the Massa client:

.. code-block::

    send_smart_contract <your_address> path/to/main.wasm 100000000 0 0 0

Where `<your_address>` should obviously be replaced by an address from your wallet. If the operation was successfully sent, you should receive a message similar to this:

.. code-block::

    Sent operation IDs:
    PHarMjNKP8kj2YEQLhkXuQuWryLGvZycTTyTdzxVhhdBCzwnn

You can now track the state of your operation using the `get_operations` command from the client:

.. code-block::

    ✔ command · get_operations NCjxpeJGN8gCMDbX1uVJBiMZhinJrE8DkxB2rUemEBkdPREhZ
    Operation's ID: NCjxpeJGN8gCMDbX1uVJBiMZhinJrE8DkxB2rUemEBkdPREhZ[in pool]
    Block's ID
        - rbkQ1eeFSVwJ7XchGMrKAhza2AEMWDrJteVr5AqmNq7wXwhre
    Id: NCjxpeJGN8gCMDbX1uVJBiMZhinJrE8DkxB2rUemEBkdPREhZ
    Signature: Gvs8XMSfkXjjmPkRVT12x1YseNv7SDYYjbk3b6G82aVCFoofXnbZ8V3jcH4Qkp3uF1cyjxY3Lyei5i5DzwaruaJn64msU
    sender: 9mvJfA4761u1qT8QwSWcJ4gTDaFP5iSgjQzKMaqTbrWCFo1QM     fee: 0     expire_period: 74942
    ExecuteSC

This command allows you to see if the operation is in the pool, in which blocks it is included and various properties.

You can also check that your smart-contract has been well deployed by fetching the events it produced with this command on the client :

.. code-block::

    get_filtered_sc_output_event caller_address=<your_address>

You should see one event with a data field which contains the address of your tic-tac-toe that has been deployed with his address:

.. code-block::

    Context: Slot: (period: 4, thread: 15) at index: 0
    On chain execution
    Block id: 2K5b2b8pFKASTtmMWPwqTTyChAKjBtJPxAreK2Yug6yqPQCshF
    Origin operation id: 2mRuf5Jv9kTGoRT11FB7x2fzQHnUhCw4rN4chB1KMn5Bq7zxf3
    Call stack: xh1fXpp7VuciaCwejMF7ufF19SWv7dFPJ7U6HiTQaeNEFBiV3

    Data: Created tictactoe smart-contract at:XKHuwsLn2A1TCEP46NQbWydAjmEzLzqAGPQcmZSc8UmjdZBJ8

The data will be different but the format should be the same.

NOTE: The tic-tac-toe is deployed to a new address each time you deploy it because in the `deploy.ts`` we use `create_sc`` to deploy the bytecode of the smart-contract to an address.
Instead we could use the `Context.set_bytecode` which will set the bytecode directly on your address. An example of GoL is using it : https://github.com/massalabs/game-of-life

.. _interacting:

Interacting with your smart-contract
====================================

We can try further our smart-contract by calling the different functions and looking at the state of the game. For this, you can create a `play.ts` under `smart-contract` repository.

play.ts
=======

.. code-block:: typescript

    import { Storage, Context, include_base64, call, print, create_sc, generate_event } from "massa-sc-std";
    import { JSON } from "json-as";
    import { PlayArgs } from "./tic_tac_toe";

    export function main(_args: string): i32 {
        // Replace by your smart-contract address
        const sc_address = "YOUR_SMART_CONTRACT_ADDRESS";
        // Start a new game
        call(sc_address, "initialize", "", 0);
        // Let's play a whole game in one smart-contract!
        call(sc_address, "play", JSON.stringify<PlayArgs>({index: 0}), 0)
        call(sc_address, "play", JSON.stringify<PlayArgs>({index: 3}), 0)
        call(sc_address, "play", JSON.stringify<PlayArgs>({index: 1}), 0)
        call(sc_address, "play", JSON.stringify<PlayArgs>({index: 4}), 0)
        call(sc_address, "play", JSON.stringify<PlayArgs>({index: 2}), 0)
        generate_event("Current player:" + Storage.get_data_for(sc_address, "currentPlayer"))
        generate_event("Game state:" + Storage.get_data_for(sc_address, "gameState"))
        generate_event("Game winner:" + Storage.get_data_for(sc_address, "gameWinner"))
        return 0;
    }

NOTE: Don't forget to change `YOUR_SMART_CONTRACT` by the address in the data of the event fetched just before.

This smart-contract initialize a new game and then play a whole game by performing a series of actions. Of course, in a real-world example this would probably be done by different players, each using a smart-contract with their specific action.

As before, you should add a line in your package.json:

.. code-block::

    "build:play": "massa-sc-scripts build-sc src/play.ts",

Then you can run `yarn run build:play`, send it to the blockchain using the `send_smart_contract` command. Once this is done and the operation is included in a block (few seconds), you should see the operations being performed by your node in the events:

.. code-block::

    Context: Slot: (period: 137, thread: 15) at index: 1
    On chain execution
    Block id: 2u6tEVN6biZQJi5AsH6aeL1WugaJnng2SjRfDU8hbbV4FZyPGc
    Origin operation id: 2AvA1sPc3uhGKtNMBMujpaeZDy35xdFkpt96RWfCBCJoKaCnDu
    Call stack: xh1fXpp7VuciaCwejMF7ufF19SWv7dFPJ7U6HiTQaeNEFBiV3

    Data: Current player:O

    Context: Slot: (period: 137, thread: 15) at index: 2
    On chain execution
    Block id: 2u6tEVN6biZQJi5AsH6aeL1WugaJnng2SjRfDU8hbbV4FZyPGc
    Origin operation id: 2AvA1sPc3uhGKtNMBMujpaeZDy35xdFkpt96RWfCBCJoKaCnDu
    Call stack: xh1fXpp7VuciaCwejMF7ufF19SWv7dFPJ7U6HiTQaeNEFBiV3

    Data: Game state:X,X,X,O,O,n,n,n,n

    Context: Slot: (period: 137, thread: 15) at index: 3
    On chain execution
    Block id: 2u6tEVN6biZQJi5AsH6aeL1WugaJnng2SjRfDU8hbbV4FZyPGc
    Origin operation id: 2AvA1sPc3uhGKtNMBMujpaeZDy35xdFkpt96RWfCBCJoKaCnDu
    Call stack: xh1fXpp7VuciaCwejMF7ufF19SWv7dFPJ7U6HiTQaeNEFBiV3

    Data: Game winner:X

The data will be different but the format should be the same.

.. _dapp:

Creating your first dApp
========================

Interacting with smart-contracts through the command line client is usually cumbersome,
and you are probably more used to interact with smart-contracts through regular websites such as `sushi.com <https://www.sushi.com/>`_.

We'll see in this part how you can host your dApp on a website and how to enable people
to interact with your smart-contract directly from the browser using the `web3 Massa library <https://github.com/massalabs/massa-web3>`_.

If you want to directly dive into the code, the front-end code is available in the html folder
of `this repository <https://github.com/massalabs/massa-sc-examples/tree/main/games/tictactoe>`_.

The front
---------

We have designed a website for the tic-tac-toe that you can find in this repository:
https://github.com/massalabs/massa-sc-examples under the folder `games/tictactoe/html`.

You will have to modify some data in order to make it work.

Setup
~~~~~

Modify the file the `baseAccount` variable in the `src/App.tsx` file with our
credentials that you get from the client using the command:

.. code-block::

    wallet_info

Also, in the same file, you have to modify the `sc_addr` variable with the address of
your tic-tac-toe that you fetched on the first event.

Then you can run :code:`npm install --leagacy-peer-deps` and :code:`yarn run start` to launch the
front and you will be able to play with tic-tac-toe.

This website use our `massa-web3 <https://github.com/massalabs/massa-web3>`_ TS library to interact
with the API and fetch the relevant informations. It can be used with a local or remote node.

.. _hosting:

Hosting your dApp on Massa decentralized web
============================================

Setup
-----

Massa offers you the possibility to host your dApp directly on a decentralized web.
This means that your website will be hosted directly on the blockchain. Decentralized
websites can then be accessed using a browser extension:

The browser extension can be downloaded `here <https://github.com/massalabs/massa-wallet>`_.
To install it on your browser, just follow the instructions of the README.md.

Once installed, to access to decentralized websites you must first connect the wallet by
clicking on `Connect wallet`.

To access to an address with the DNS, you have to use the prefix `massa://` in the URL bar.
For example you should have access to the following websites:

- `massa://gol` which is a Game-of-Life on the blockchain. You can click to interact with it.
- `massa://ttt` a tic-tac-toe.

If you have access to those websites it's that your extension is well configured.

In this tutorial we will show you how to deploy a decentralized website and how to
setup the Massa DNS to be able to access your website using the wallet extension.

Uploading your website
----------------------

Now that you have the extension well configured you can deploy your superb website of tic-tac-toe
on the blockchain.

First of all you have to turn your website into bytecode that can be inserted in the blockchain.
Here is the list of the command you need to make under the `tictactoe/html` folder:

.. code-block::

    yarn run build
    cd build && zip -r site.zip * && cd .. && npx massa-sc-scripts build-website-sc build/site.zip

Now you can upload it on the blockchain running the following command on the client:

.. code-block::

    send_smart_contract <your_address> /path/to/tictactoe/html/build/website.wasm 100000000 0 0 0

Setting the DNS
---------------

Now your website should be uploaded on the blockchain. We'll now want to add a DNS address
to our smart-contract. This will allow us to access to our decentralized website using a
regular address and not the address of the smart-contract which is a hash and thus not really convenient.

To access it on the browser you have to link it to a DNS entry.
To add a DNS entry you can use the following helper command in the folder of your client:

.. code-block::

    ./massa-client call_smart_contract <your address> 2R4zRvGc5GcX4eCWrM5zsboFKodCUuWa7X8biiDBQMoLohwH4N setResolver '{"name": "<name_of_your_website>", "address": "<your_address>"}' 1000000000 0 0 0

Where you should replace `<name_of_your_website>` by the address that you want for your website,
and `<your address>` by the wallet address that you used in the previous steps.

Accessing your website
----------------------

Note that before accessing to a website you have to make sure you are connected in the extension. 
To be connected go on the icon of the extension and click on it if you have the `Connect wallet`
button then click it otherwise you are already connected.

You can now type `massa://<your_website_name>` in the address bar of your navigator to be able to access to your website.

When you will code your proper website you can follow the steps just above, re-deploy over the
current example and keep your DNS entry.

Going further
=============

- You can test smart-contracts locally using the `Massa smart-contract tester <https://github.com/massalabs/massa-sc-tester>`_.
