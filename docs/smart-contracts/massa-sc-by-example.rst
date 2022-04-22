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

You can find the complete project `here <https://github.com/massalabs/tictactoe-poc>`__.

Prerequisites
=============

Smart-contracts are written in `Assembly Script <https://www.assemblyscript.org/>`_, and so we’ll assume that you have some familiarity with it, but you should be able to follow along even if you’re coming from a different programming language. We’ll also assume that you’re familiar with programming concepts like functions, objects, arrays, and to a lesser extent, classes.

For the decentralized website part, we'll assume that you have some familiarity with HTML and JavaScript. If you want to have more details, you can follow `this great tutorial from React <https://reactjs.org/tutorial/tutorial.html>`_ from which the dApp is heavily inspired from.

.. _writing-sc:

Writing your smart-contract
===========================

Smart-contract in Massa are written in `Assembly Script <https://www.assemblyscript.org/>`_ and then compiled to `WebAssembly <https://webassembly.org/>`_ (WASM). We chose WebAssembly as it is efficient and can be compiled from several languages, including Assembly Script. If you want to have more details on how the Massa smart-contract engine was built, have a look `here <https://github.com/massalabs/massa-sc-runtime/issues/93>`__.

Setup
-----

You need `node`, `yarn` and `npx` to initialize the project!

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

The `initialize` function is used to start a new tic-tac-toe game. This function is used to instantiate the different variables that will be used to track the state of the game: `currentPlayer`, `gameState` and `gameWinner`. Note that smart-contract data is stored in a hash map where keys and values must be string. For more details, we refer to the documentation: TODO.

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

The `_checkWin` function is used to check whether the game ended or not. Private, as it does not use the `export` prefix, it cannot be called by anyone. It can only be called internally by the smart-contract.

main.ts

.. code-block:: typescript

    import { Storage, Context, include_base64, call, print, create_sc } from "massa-sc-std";
    import { JSON } from "json-as";
    import { PlayArgs } from "./tic_tac_toe";

    function createContract(): string {
        const bytes = include_base64('./build/tictactoe_play.wasm');
        const sc_address = create_sc(bytes);
        return sc_address;
    }

    export function main(_args: string): i32 {
        const sc_address = createContract();
        print("Created tictactoe smart-contract at:" + sc_address);
        return 0;
    }

Compiling your smart-contract
-----------------------------

Smart-contract can be compiled using the `massa-sc-scripts` command: TODO.

.. _sending-sc:

Putting your smart-contract on the blockchain
=============================================

We'll now turn to the process of putting the smart-contract on the Massa blockchain.

Setup
~~~~~

To put your smart-contract on the Massa blockchain, you need to run a node and use the client. If this is something that you have never done, all the steps are detailed in the `tutorials <https://github.com/massalabs/massa#tutorials-to-join-the-testnet>`_.

To be able to be able to read the different prints when you execute the smart-contracts, you should set the logging level to debug. Edit (or create) the `massa/massa-node/config/config.toml` file and add the following line:

.. code-block:: toml

    [logging]
        level = 3

Sending the smart-contract
==========================

Sending the smart-contract to the Massa blockchain is done using the `send_smart_contract` from the Massa client:

.. code-block::

    send_smart_contract <your_address> main.wasm 1000000 0 0 0

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

Once the operation is included in a block, it is immediately executed. Now, looking at the logs of the node, you should see something similar to this:

.. code-block::

    2022-03-17T16:18:42.002581Z DEBUG massa_execution_worker::interface_impl: SC print: Initialized, address:yqQMHYoWoD569AzuqT97HW8bTuZu44yx5z6W1wRtQP9u715r4

.. _interacting:

Interacting with your smart-contract
====================================

We can try further our smart-contract by calling the different functions and looking at the state of the game. For this, we create a new smart-contract `main.ts`.

main.ts
=======

.. code-block:: typescript

    import { Storage, Context, include_base64, call, print, create_sc } from "massa-sc-std";
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
        print("Current player:" + Storage.get_data_for(sc_address, "currentPlayer"))
        print("Game state:" + Storage.get_data_for(sc_address, "gameState"))
        print("Game winner:" + Storage.get_data_for(sc_address, "gameWinner"))
        return 0;
    }

This smart-contract initialize a new game and then play a whole game by performing a series of actions. Of course, in a real-world example this would probably be done by different players, each using a smart-contract with their specific action.

As before, you should compile your smart-contract, send it to the blockchain using the `send_smart_contract` command. Once this is done and the operation is included in a block, you should see the operations being performed by your node in the logs:

.. code-block::

    2022-03-17T16:18:42.015501Z DEBUG massa_execution_worker::interface_impl: SC print: Current player:O
    2022-03-17T16:18:42.015528Z DEBUG massa_execution_worker::interface_impl: SC print: Game state:X,X,X,O,O,n,n,n,n
    2022-03-17T16:18:42.015543Z DEBUG massa_execution_worker::interface_impl: SC print: Game winner:X

.. _dapp:

Creating your first dApp
========================

Interacting with smart-contracts through the command line client is usually cumbersome, and you are probably more used to interact with smart-contracts through regular websites such as `sushi.com <https://www.sushi.com/>`_.

We'll see in this part how you can host your dApp on a website and how to enable people to interact with your smart-contract directly from the browser using the web3 Massa library.

The front
---------

Now let's start building out the client-side application that will talk to our smart-contract. We'll do this by modifying the HTML and JavaScript files made by `React for their tutorial <https://reactjs.org/tutorial/tutorial.html>`_. What we'll essentially do is replace the standard functions used to play the game and store the state of the game with smart-contracts actions.

You do not have to be a front-end expert to follow along with this part of the tutorial. I have intentionally kept the HTML and JavaScript code simple, and we will not spend much time focusing on it.

We'll be using the front end made by `React for their tutorial <https://reactjs.org/tutorial/tutorial.html>`_.

.. _hosting:

Hosting your dApp on Massa decentralized web
============================================

Going further
=============

- You can test smart-contracts locally using the `Massa smart-contract tester <https://github.com/massalabs/massa-sc-tester>`_.
