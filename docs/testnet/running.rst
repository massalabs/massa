==============
Running a node
==============

From binaries
=============

Simply run the binaries you downloaded in the previous step:
Open the `massa-node` folder and run the `massa-node` excutable
Open the `massa-client` folder and run the `massa-client` excutable

From source code
================

On Ubuntu / MacOS
-----------------

**Start the node**

On a first window:

.. code-block:: bash

    cd massa/massa-node/

Launch the node, on Ubuntu:

.. code-block:: bash

    RUST_BACKTRACE=full cargo run --release |& tee logs.txt

**Or,** on macOS:

.. code-block:: bash

    RUST_BACKTRACE=full cargo run --release > logs.txt 2>&1

You should leave the window opened.

**Start the client**

On a second window:

.. code-block:: bash

    cd massa/massa-client/

Then:

.. code-block:: bash

    cargo run --release

Please wait until the directories are built before moving to the next step.

On Windows
----------

**Start the Node**

- Open Windows Power Shell or Command Prompt on a first window
    - Type: :code:`cd massa`
    - Type: :code:`cd massa-node`
    - Type: :code:`cargo run --release`

You should leave the window opened.

**Start the Client**

- Open Windows Power Shell or Command Prompt on a second window
    - Type: :code:`cd massa`
    - Type: :code:`cd massa-client`
    - Type: :code:`cargo run --release`

Please wait until the directories are built before moving to the next step.

Next step
=========

TODO:
- `Creating a wallet <https://github.com/massalabs/massa/wiki/wallet>`_
